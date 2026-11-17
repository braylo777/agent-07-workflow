//! A durable, checkpointed workflow engine: each step's output is saved to KV, failed steps retry with
//! exponential backoff, and a run can be resumed from its last completed step.
use regex::Regex;
use serde_json::{json, Value};
use std::future::Future;

pub const MODEL: &str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";
pub const MAX_ATTEMPTS: u32 = 3;
pub const STEP_NAMES: [&str; 4] = ["classify", "pain_points", "score", "draft_email"];

#[allow(async_fn_in_trait)]
pub trait Ai {
    async fn run(&self, model: &str, input: Value) -> Result<Value, String>;
}
#[allow(async_fn_in_trait)]
pub trait Kv {
    async fn get(&self, key: &str) -> Option<String>;
    async fn put(&self, key: &str, val: &str, ttl: Option<u64>);
}

pub fn text(r: &Value) -> String {
    match r.get("response") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => r.to_string(),
        Some(v) => v.to_string(),
    }
}
pub fn parse_json(s: &str) -> Option<Value> {
    Regex::new(r"[\[{][\s\S]*[\]}]").unwrap().find(s).and_then(|m| serde_json::from_str(m.as_str()).ok())
}
pub fn cap(s: Option<&str>, n: usize) -> bool {
    matches!(s, Some(s) if !s.trim().is_empty() && s.chars().count() <= n)
}

pub fn iso_ms(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", if m <= 2 { y + 1 } else { y }, m, d, rem / 3_600_000, rem % 3_600_000 / 60_000, rem % 60_000 / 1000, rem % 1000)
}

fn js(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => "undefined".into(),
        Some(o) => o.to_string(),
    }
}

async fn ask<A: Ai>(ai: &A, system: &str, user: &str, max: u32) -> Result<String, String> {
    let r = ai.run(MODEL, json!({"messages":[{"role":"system","content":system},{"role":"user","content":user}],"max_tokens":max})).await?;
    Ok(text(&r))
}

async fn step<A: Ai>(name: &str, ai: &A, run: &Value) -> Result<Value, String> {
    let lead = run["input"]["lead"].as_str().unwrap_or("");
    let out = &run["out"];
    Ok(match name {
        "classify" => parse_json(&ask(ai, "Classify this business lead. Reply ONLY JSON: {\"industry\":string,\"size\":\"solo|small|mid|large\"}", lead, 80).await?).unwrap_or(json!({"industry":"unknown","size":"unknown"})),
        "pain_points" => parse_json(&ask(ai, "List the top 3 likely operational pain points AI could solve for this business. Reply ONLY a JSON array of strings.", &format!("{}\nIndustry: {}", lead, js(out["classify"].get("industry"))), 200).await?).unwrap_or(json!([])),
        "score" => {
            let mut o = serde_json::Map::new();
            o.insert("lead".into(), json!(lead));
            for (k, v) in out.as_object().unwrap() {
                o.insert(k.clone(), v.clone());
            }
            parse_json(&ask(ai, "Score this lead 0-100 for fit with an AI automation agency. Reply ONLY JSON: {\"score\":number,\"reason\":string}", &Value::Object(o).to_string(), 120).await?).unwrap_or(json!({"score":0,"reason":"unparseable"}))
        }
        _ => {
            let mut o = serde_json::Map::new();
            o.insert("lead".into(), json!(lead));
            if let Some(p) = out.get("pain_points") {
                o.insert("pains".into(), p.clone());
            }
            json!(ask(ai, "Write a short, friendly first-touch email (max 90 words) offering to solve the pain points. No placeholders except [Name].", &Value::Object(o).to_string(), 250).await?)
        }
    })
}

pub async fn persist<K: Kv>(kv: &K, run: &mut Value, now: i64) {
    run["updated"] = json!(iso_ms(now));
    kv.put(&format!("run:{}", run["id"].as_str().unwrap_or("")), &run.to_string(), Some(86400 * 3)).await;
}
pub async fn get_run<K: Kv>(kv: &K, id: &str) -> Option<Value> {
    kv.get(&format!("run:{id}")).await.and_then(|r| serde_json::from_str(&r).ok())
}
pub fn new_run(input: Value, id: &str, created_ms: i64) -> Value {
    json!({"id": id, "input": input, "out": {}, "log": [], "status": "queued", "created": iso_ms(created_ms)})
}

/// `sleep(ms)` and `now()` (ms) are injected: tests use no-ops, the Worker uses a real delay and Date.now.
pub async fn execute<A: Ai, K: Kv, S, Fut>(ai: &A, kv: &K, mut run: Value, backoff: u64, sleep: S, now: &dyn Fn() -> i64) -> Value
where
    S: Fn(u64) -> Fut,
    Fut: Future<Output = ()>,
{
    run["status"] = json!("running");
    for name in STEP_NAMES {
        if run["out"].get(name).is_some() {
            continue; // checkpoint: already done
        }
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let t0 = now();
            let res = if run["input"]["fail_step"].as_str() == Some(name) && attempt == 1 { Err("injected failure (demo)".to_string()) } else { step(name, ai, &run).await };
            match res {
                Ok(v) => {
                    run["out"][name] = v;
                    run["log"].as_array_mut().unwrap().push(json!({"step": name, "attempt": attempt, "ok": true, "ms": now() - t0}));
                    break;
                }
                Err(e) => {
                    run["log"].as_array_mut().unwrap().push(json!({"step": name, "attempt": attempt, "ok": false, "error": e, "ms": now() - t0}));
                    if attempt >= MAX_ATTEMPTS {
                        run["status"] = json!("failed");
                        persist(kv, &mut run, now()).await;
                        return run;
                    }
                    sleep(backoff * 2u64.pow(attempt - 1)).await;
                }
            }
        }
        persist(kv, &mut run, now()).await; // checkpoint after every step
    }
    run["status"] = json!("complete");
    persist(kv, &mut run, now()).await;
    run
}

/// Pure router: (status, json); status 0 = serve the page. `new_id`/`now` are injected.
pub async fn handle<A: Ai, K: Kv, S, Fut>(ai: &A, kv: &K, method: &str, path: &str, body: &Value, new_id: &str, backoff: u64, sleep: S, now: &dyn Fn() -> i64) -> (u16, Value)
where
    S: Fn(u64) -> Fut,
    Fut: Future<Output = ()>,
{
    let re = Regex::new(r"^/api/runs/([a-z0-9-]{4,40})(/resume)?$").unwrap();
    let m = re.captures(path);
    if method == "GET" && path == "/api/steps" {
        return (200, json!(STEP_NAMES));
    }
    if method == "GET" {
        if let Some(c) = &m {
            if c.get(2).is_none() {
                return match get_run(kv, &c[1]).await {
                    Some(r) => (200, r),
                    None => (404, json!({"error": "not found"})),
                };
            }
        }
    }
    if method == "POST" && (path == "/api/runs" || m.as_ref().map_or(false, |c| c.get(2).is_some())) {
        let run = if let Some(c) = &m {
            match get_run(kv, &c[1]).await {
                None => return (404, json!({"error": "not found"})),
                Some(r) if r["status"] == "complete" => return (200, r),
                Some(r) => r,
            }
        } else {
            let lead = body.get("lead").and_then(|v| v.as_str());
            if !cap(lead, 500) {
                return (400, json!({"error": "lead required (<=500 chars)"}));
            }
            let fs = body.get("fail_step").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
            if let Some(f) = fs {
                if !STEP_NAMES.contains(&f) {
                    return (400, json!({"error": "unknown fail_step"}));
                }
            }
            let mut input = json!({"lead": lead.unwrap()});
            if let Some(f) = fs {
                input["fail_step"] = json!(f);
            }
            new_run(input, new_id, now())
        };
        return (200, execute(ai, kv, run, backoff, sleep, now).await);
    }
    (0, Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    #[derive(Default)]
    struct Map(RefCell<HashMap<String, String>>);
    impl Kv for Map {
        async fn get(&self, k: &str) -> Option<String> { self.0.borrow().get(k).cloned() }
        async fn put(&self, k: &str, v: &str, _t: Option<u64>) { self.0.borrow_mut().insert(k.into(), v.into()); }
    }
    struct Ai1<F: Fn(&str) -> Result<String, String>>(F, RefCell<Vec<String>>);
    impl<F: Fn(&str) -> Result<String, String>> Ai for Ai1<F> {
        async fn run(&self, _m: &str, inp: Value) -> Result<Value, String> {
            let s = inp["messages"][0]["content"].as_str().unwrap().to_string();
            self.1.borrow_mut().push(s.split(' ').next().unwrap().to_string());
            (self.0)(&s).map(|r| json!({"response": r}))
        }
    }
    fn ai<F: Fn(&str) -> Result<String, String>>(f: F) -> Ai1<F> { Ai1(f, RefCell::new(vec![])) }
    fn ok(s: &str) -> Result<String, String> {
        Ok(if s.starts_with("Classify") { "Sure: {\"industry\":\"hvac\",\"size\":\"small\"}".into() } else if s.starts_with("List") { "[\"a\",\"b\",\"c\"]".into() } else if s.starts_with("Score") { "{\"score\":70,\"reason\":\"r\"}".into() } else { "Hi [Name]".into() })
    }
    async fn nosleep(_ms: u64) {}
    fn clk() -> impl Fn() -> i64 { let c = Cell::new(1_700_000_000_000i64); move || { c.set(c.get() + 3); c.get() } }
    fn exec<A: Ai>(a: &A, kv: &Map, run: Value) -> Value { block_on(execute(a, kv, run, 1, nosleep, &clk())) }

    #[test]
    fn runs_all_steps_and_checkpoints() {
        let kv = Map::default();
        let r = exec(&ai(ok), &kv, new_run(json!({"lead": "Joe's Plumbing, Austin"}), "abcd1234", 0));
        assert_eq!(r["status"], "complete");
        assert_eq!(r["out"]["score"]["score"], 70);
        assert_eq!(r["out"]["classify"]["industry"], "hvac");
        assert_eq!(r["log"].as_array().unwrap().len(), 4);
        assert_eq!(kv.0.borrow().len(), 1);
    }
    #[test]
    fn injected_failure_is_retried_and_unparseable_falls_back() {
        let r = exec(&ai(ok), &Map::default(), new_run(json!({"lead":"x","fail_step":"score"}), "abcd1234", 0));
        let oks: Vec<bool> = r["log"].as_array().unwrap().iter().filter(|l| l["step"] == "score").map(|l| l["ok"].as_bool().unwrap()).collect();
        assert_eq!(r["status"], "complete");
        assert_eq!(oks, [false, true]);
        let r = exec(&ai(|_| Ok("no json here".into())), &Map::default(), new_run(json!({"lead":"x"}), "abcd1234", 0));
        assert_eq!(r["status"], "complete");
        assert_eq!(r["out"]["score"]["reason"], "unparseable");
        assert_eq!(r["out"]["pain_points"], json!([]));
    }
    #[test]
    fn resume_skips_completed_steps() {
        let a = ai(ok);
        let mut run = new_run(json!({"lead":"x"}), "abcd1234", 0);
        run["out"] = json!({"classify":{"industry":"a"},"pain_points":["p"]});
        exec(&a, &Map::default(), run);
        assert_eq!(*a.1.borrow(), ["Score", "Write"]);
    }
    #[test]
    fn failing_every_attempt_then_resume() {
        let broken = Cell::new(true);
        let a = ai(|s| if broken.get() && s.starts_with("Score") { Err("down".into()) } else { ok(s) });
        let kv = Map::default();
        let run = exec(&a, &kv, new_run(json!({"lead":"x"}), "abcd1234", 0));
        assert_eq!(run["status"], "failed");
        assert_eq!(run["log"].as_array().unwrap().iter().filter(|l| l["step"] == "score").count() as u32, MAX_ATTEMPTS);
        assert!(run["out"].get("draft_email").is_none());
        broken.set(false);
        let saved = block_on(get_run(&kv, "abcd1234")).unwrap();
        let r2 = exec(&a, &kv, saved);
        assert_eq!(r2["status"], "complete");
        assert_eq!(r2["log"].as_array().unwrap().iter().filter(|l| l["step"] == "classify").count(), 1);
    }
    #[test]
    fn http_router() {
        let kv = Map::default();
        let a = ai(ok);
        let go = |m: &str, p: &str, b: Value| block_on(handle(&a, &kv, m, p, &b, "abcd1234", 1, nosleep, &clk()));
        assert_eq!(go("GET", "/api/steps", json!({})).1, json!(STEP_NAMES));
        assert_eq!(go("POST", "/api/runs", json!({})).0, 400);
        assert_eq!(go("POST", "/api/runs", json!({"lead":"x","fail_step":"nope"})).0, 400);
        assert_eq!(go("GET", "/api/runs/abcd1234", json!({})).0, 404);
        let (s, r) = go("POST", "/api/runs", json!({"lead":"x"}));
        assert_eq!((s, r["status"].as_str().unwrap()), (200, "complete"));
        assert_eq!(go("POST", "/api/runs/abcd1234/resume", json!({})).1["id"], "abcd1234");
        assert_eq!(go("GET", "/api/runs/abcd1234", json!({})).0, 200);
        assert!(parse_json("nope").is_none());
        assert!(cap(Some("hi"), 500) && !cap(Some(""), 500) && !cap(Some(&"x".repeat(501)), 500));
    }
}
