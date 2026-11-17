// Parity helper: same scripted scenarios as the legacy JS harness; prints JSON.
use agent_07_workflow::workflow::*;
use futures::executor::block_on;
use serde_json::{json, Value};
use std::{cell::{Cell, RefCell}, collections::HashMap};

#[derive(Default)]
struct Map(RefCell<HashMap<String, String>>);
impl Kv for Map {
    async fn get(&self, k: &str) -> Option<String> { self.0.borrow().get(k).cloned() }
    async fn put(&self, k: &str, v: &str, _t: Option<u64>) { self.0.borrow_mut().insert(k.into(), v.into()); }
}
struct F<'a> { mode: &'a str, broken: &'a Cell<bool> }
impl<'a> Ai for F<'a> {
    async fn run(&self, _m: &str, i: Value) -> Result<Value, String> {
        let s = i["messages"][0]["content"].as_str().unwrap();
        let u = i["messages"][1]["content"].as_str().unwrap();
        if self.mode == "garbage" { return Ok(json!({"response":"no json here"})); }
        if self.broken.get() && s.starts_with("Score") { return Err("down".into()); }
        let r = if s.starts_with("Classify") { if self.mode == "noind" { "{\"size\":\"small\"}".to_string() } else { "Sure: {\"industry\":\"hvac\",\"size\":\"small\"}".to_string() } }
            else if s.starts_with("List") { format!("[\"a\",\"b\",\"c\"] {}", u.replace('\n', "|")) }
            else if s.starts_with("Score") { format!("{{\"score\":70,\"reason\":\"{}\"}}", u.encode_utf16().count()) }
            else { format!("Hi [Name] {u}") };
        Ok(json!({"response": r}))
    }
}
async fn ns(_: u64) {}
fn strip(r: &Value) -> Value {
    json!({"status": r["status"], "input": r["input"], "out": r["out"], "log": r["log"].as_array().unwrap().iter().map(|l| { let mut o = json!({"step": l["step"], "attempt": l["attempt"], "ok": l["ok"]}); if l.get("error").is_some() { o["error"] = l["error"].clone(); } o }).collect::<Vec<_>>()})
}
fn main() {
    let now = || 1_700_000_000_000i64;
    let no = Cell::new(false);
    let mut out = vec![];
    for (mode, input) in [("ok", json!({"lead":"Joe's Plumbing, Austin é"})), ("ok", json!({"lead":"x","fail_step":"score"})), ("garbage", json!({"lead":"y"})), ("noind", json!({"lead":"z"}))] {
        let r = block_on(execute(&F { mode, broken: &no }, &Map::default(), new_run(input, "abcd1234", 0), 1, ns, &now));
        out.push(strip(&r));
    }
    let broken = Cell::new(true);
    let kv = Map::default();
    let ai = F { mode: "ok", broken: &broken };
    let r1 = block_on(execute(&ai, &kv, new_run(json!({"lead":"w"}), "abcd1234", 0), 1, ns, &now));
    out.push(strip(&r1));
    broken.set(false);
    let saved = block_on(get_run(&kv, "abcd1234")).unwrap();
    out.push(strip(&block_on(execute(&ai, &kv, saved, 1, ns, &now))));
    println!("{}", serde_json::to_string(&out).unwrap());
}
