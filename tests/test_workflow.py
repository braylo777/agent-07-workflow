import json
from agent07.workflow import execute, new_run, get_run, handle, STEP_NAMES, MAX_ATTEMPTS, parse_json, cap


class KV:
    def __init__(self): self.m = {}
    def get(self, k): return self.m.get(k)
    def put(self, k, v, ttl=None): self.m[k] = v


NO = dict(backoff=1, sleep=lambda ms: None)


def ai(on_call=lambda s: None):
    def run(_m, inp):
        s = inp["messages"][0]["content"]; on_call(s)
        if s.startswith("Classify"): return {"response": '{"industry":"plumbing","size":"small"}'}
        if s.startswith("List"): return {"response": '["missed calls","scheduling","quotes"]'}
        if s.startswith("Score"): return {"response": '{"score":82,"reason":"high call volume"}'}
        return {"response": "Hi [Name], ..."}
    return run


def test_runs_all_steps_and_checkpoints():
    kv = KV()
    r = execute(ai(), kv, new_run({"lead": "Joe's Plumbing, Austin"}), **NO)
    assert r["status"] == "complete" and r["out"]["score"]["score"] == 82 and len(r["log"]) == len(STEP_NAMES) and len(kv.m) == 1


def test_injected_failure_is_retried():
    r = execute(ai(), KV(), new_run({"lead": "x", "fail_step": "score"}), **NO)
    assert r["status"] == "complete" and [l["ok"] for l in r["log"] if l["step"] == "score"] == [False, True]


def test_resume_skips_completed_steps():
    calls = []
    run = new_run({"lead": "x"}); run["out"] = {"classify": {"industry": "a"}, "pain_points": ["p"]}
    execute(ai(lambda s: calls.append(s.split(" ")[0])), KV(), run, **NO)
    assert calls == ["Score", "Write"]


def test_step_order_fixed():
    assert STEP_NAMES == ["classify", "pain_points", "score", "draft_email"]


def ok_ai(_m, i):
    s = i["messages"][0]["content"]
    if s.startswith("Classify"): return {"response": 'Sure: {"industry":"hvac","size":"small"}'}
    if s.startswith("List"): return {"response": '["a","b","c"]'}
    if s.startswith("Score"): return {"response": '{"score":70,"reason":"r"}'}
    return {"response": "Hi [Name]"}


def test_json_extracted_from_chatty_output():
    assert execute(ok_ai, KV(), new_run({"lead": "x"}), **NO)["out"]["classify"]["industry"] == "hvac"


def test_unparseable_falls_back():
    r = execute(lambda m, i: {"response": "no json here"}, KV(), new_run({"lead": "x"}), **NO)
    assert r["status"] == "complete" and r["out"]["score"]["reason"] == "unparseable" and r["out"]["pain_points"] == []


def test_failing_every_attempt_marks_failed():
    def a(_m, i):
        if i["messages"][0]["content"].startswith("Score"): raise RuntimeError("boom")
        return ok_ai(_m, i)
    r = execute(a, KV(), new_run({"lead": "x"}), **NO)
    assert r["status"] == "failed" and len([l for l in r["log"] if l["step"] == "score"]) == MAX_ATTEMPTS and "draft_email" not in r["out"]


def test_failed_run_resumes_when_fault_clears():
    kv, st = KV(), {"broken": True}
    def a(_m, i):
        if st["broken"] and i["messages"][0]["content"].startswith("Score"): raise RuntimeError("down")
        return ok_ai(_m, i)
    run = execute(a, kv, new_run({"lead": "x"}), **NO)
    assert run["status"] == "failed"
    st["broken"] = False
    r2 = execute(a, kv, get_run(kv, run["id"]), **NO)
    assert r2["status"] == "complete" and len([l for l in r2["log"] if l["step"] == "classify"]) == 1


def test_persisted_under_id():
    kv = KV()
    r = execute(ok_ai, kv, new_run({"lead": "x"}), **NO)
    assert get_run(kv, r["id"])["status"] == "complete"


def test_helpers_and_http():
    assert parse_json("nope", 1) == 1 and cap("hi") and not cap("") and not cap("x" * 501)
    kv = KV()
    assert handle(ok_ai, kv, "GET", "/api/steps")[1] == STEP_NAMES
    assert handle(ok_ai, kv, "POST", "/api/runs", {})[0] == 400
    assert handle(ok_ai, kv, "POST", "/api/runs", {"lead": "x", "fail_step": "nope"})[0] == 400
    assert handle(ok_ai, kv, "GET", "/api/runs/abcd1234")[0] == 404
    s, r = handle(ok_ai, kv, "POST", "/api/runs", {"lead": "x"}, **NO)
    assert s == 200 and r["status"] == "complete" and handle(ok_ai, kv, "POST", f"/api/runs/{r['id']}/resume")[1]["id"] == r["id"]
