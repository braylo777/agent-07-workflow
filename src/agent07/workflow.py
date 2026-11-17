"""A durable, checkpointed workflow engine: each step's output is saved to KV, failed steps retry with
exponential backoff, and a run can be resumed from its last completed step."""
import json, re, time, uuid
from datetime import datetime, timezone

MODEL = "@cf/meta/llama-3.3-70b-instruct-fp8-fast"
MAX_ATTEMPTS = 3
STEP_NAMES = ["classify", "pain_points", "score", "draft_email"]


def text(r):
    v = r.get("response") if isinstance(r, dict) else None
    return v if isinstance(v, str) else json.dumps(v if v is not None else r, separators=(",", ":"), ensure_ascii=False)


def parse_json(s, fallback=None):
    m = re.search(r"[\[{][\s\S]*[\]}]", str(s))
    try:
        return json.loads(m.group(0)) if m else fallback
    except ValueError:
        return fallback


def cap(s, n=500):
    return isinstance(s, str) and bool(s.strip()) and len(s) <= n


dumps = lambda o: json.dumps(o, separators=(",", ":"), ensure_ascii=False)
iso = lambda ms=None: datetime.fromtimestamp((time.time() * 1000 if ms is None else ms) / 1000, timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def _js(v):
    return "undefined" if v is None else str(v)


def _get(o, k):
    return o.get(k) if isinstance(o, dict) else None


def _ask(ai, system, user, mx):
    return text(ai(MODEL, {"messages": [{"role": "system", "content": system}, {"role": "user", "content": user}], "max_tokens": mx}))


def _step(name, ai, s):
    inp, out = s["input"], s["out"]
    if name == "classify":
        return parse_json(_ask(ai, 'Classify this business lead. Reply ONLY JSON: {"industry":string,"size":"solo|small|mid|large"}', inp["lead"], 80), {"industry": "unknown", "size": "unknown"})
    if name == "pain_points":
        return parse_json(_ask(ai, "List the top 3 likely operational pain points AI could solve for this business. Reply ONLY a JSON array of strings.", f"{inp['lead']}\nIndustry: {_js(_get(out['classify'], 'industry'))}", 200), [])
    if name == "score":
        return parse_json(_ask(ai, 'Score this lead 0-100 for fit with an AI automation agency. Reply ONLY JSON: {"score":number,"reason":string}', dumps({"lead": inp["lead"], **out}), 120), {"score": 0, "reason": "unparseable"})
    return _ask(ai, "Write a short, friendly first-touch email (max 90 words) offering to solve the pain points. No placeholders except [Name].", dumps({"lead": inp["lead"], "pains": out.get("pain_points")}), 250)


def persist(kv, run, now=None):
    run["updated"] = iso(now)
    kv.put(f"run:{run['id']}", dumps(run), 86400 * 3)


def get_run(kv, rid):
    raw = kv.get(f"run:{rid}")
    return json.loads(raw) if raw else None


def new_run(inp, rid=None, created=None):
    return {"id": rid or str(uuid.uuid4())[:8], "input": inp, "out": {}, "log": [], "status": "queued", "created": created or iso()}


def execute(ai, kv, run, backoff=250, sleep=lambda ms: time.sleep(ms / 1000), now=lambda: time.time() * 1000):
    run["status"] = "running"
    for name in STEP_NAMES:
        if name in run["out"]:
            continue  # checkpoint: already done
        attempt = 0
        while True:
            attempt += 1
            t0 = now()
            try:
                if run["input"].get("fail_step") == name and attempt == 1:
                    raise RuntimeError("injected failure (demo)")
                run["out"][name] = _step(name, ai, run)
                run["log"].append({"step": name, "attempt": attempt, "ok": True, "ms": round(now() - t0)})
                break
            except Exception as e:
                run["log"].append({"step": name, "attempt": attempt, "ok": False, "error": str(e), "ms": round(now() - t0)})
                if attempt >= MAX_ATTEMPTS:
                    run["status"] = "failed"
                    persist(kv, run)
                    return run
                sleep(backoff * 2 ** (attempt - 1))
        persist(kv, run)  # checkpoint after every step
    run["status"] = "complete"
    persist(kv, run)
    return run


def handle(ai, kv, method, path, body=None, **kw):
    """Pure router: (status, json); status 0 = serve the page."""
    m = re.fullmatch(r"/api/runs/([a-z0-9-]{4,40})(/resume)?", path)
    if method == "GET" and path == "/api/steps":
        return 200, list(STEP_NAMES)
    if method == "GET" and m and not m.group(2):
        r = get_run(kv, m.group(1))
        return (200, r) if r else (404, {"error": "not found"})
    if method == "POST" and (path == "/api/runs" or (m and m.group(2))):
        if m:
            run = get_run(kv, m.group(1))
            if not run:
                return 404, {"error": "not found"}
            if run["status"] == "complete":
                return 200, run
        else:
            b = body if isinstance(body, dict) else {}
            if not cap(b.get("lead")):
                return 400, {"error": "lead required (<=500 chars)"}
            if b.get("fail_step") and b["fail_step"] not in STEP_NAMES:
                return 400, {"error": "unknown fail_step"}
            run = new_run({"lead": b["lead"], **({"fail_step": b["fail_step"]} if b.get("fail_step") else {})})
        try:
            return 200, execute(ai, kv, run, **kw)
        except Exception as e:
            return 502, {"error": "Workflow error: " + str(e), "id": run["id"]}
    return 0, None
