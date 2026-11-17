"""Live eval suite: hits the deployed Worker (real Workers AI calls). Run: BASE=https://... python3 evals/run.py"""
import json, math, os, re, time, urllib.error, urllib.request
from datetime import datetime, timezone
from pathlib import Path

NAME = "agent-07-workflow"
BASE = os.environ.get("BASE", f"https://{NAME}.braylo777.workers.dev")
GAP = float(os.environ.get("GAP_MS", "13000")) / 1000  # 5 req/min rate limit on the demo
here = Path(__file__).parent
lat, last = [], [0.0]


def call(method, path, body=None):
    w = last[0] + GAP - time.time()
    if w > 0:
        time.sleep(w)
    for _ in range(3):
        last[0] = time.time(); t0 = time.time()
        req = urllib.request.Request(BASE + path, None if method == "GET" else json.dumps(body).encode(), {"content-type": "application/json"}, method=method)
        try:
            r = urllib.request.urlopen(req, timeout=90); status = r.status; raw = r.read()
        except urllib.error.HTTPError as e:
            status = e.code; raw = e.read()
        ms = round((time.time() - t0) * 1000)
        try:
            j = json.loads(raw)
        except ValueError:
            j = {}
        if status == 429:
            time.sleep(61); continue
        lat.append(ms)
        return {"status": status, "ms": ms, "j": j}
    raise RuntimeError("rate limited 3x")


pct = lambda a, p: sorted(a)[max(0, math.ceil(p / 100 * len(a)) - 1)] if a else None
ratio = lambda n, d: {"passed": n, "total": d, "rate": n / d if d else None}
LEADS = [
    "Rodriguez Roofing, 6 trucks, Dallas TX. Owner misses calls during jobs and quotes take days.",
    "Bright Smile Dental, 2 locations in Phoenix, front desk overwhelmed with reschedules.",
    "Ace Plumbing, solo plumber in Austin, does invoicing by hand at night.",
    "GreenLeaf Landscaping, 15 crew members, Atlanta, scheduling via group texts.",
    "Peak HVAC, 40 techs across Denver, after-hours calls go to voicemail.",
    "Lotus Day Spa, Miami, no-shows are 20% of bookings.",
    "Main Street Bakery, Portland, owner answers every catering inquiry by email.",
    "Summit Law Group, 8 attorneys in Chicago, intake forms are paper.",
    "QuickFix Auto Repair, 3 bays, Houston, customers call constantly for status updates.",
    "Coastal Realty, 25 agents in San Diego, leads go cold before follow-up.",
    "Iron Gym, 1 location in Tampa, member cancellations are handled manually.",
    "Paws & Claws Vet Clinic, Seattle, phone lines jammed every morning.",
    "Sparkle Cleaning Co, 12 cleaners, Nashville, quotes are done on paper.",
    "BrightPath Tutoring, online, parents email to book and pay.",
    "Elite Electric, 5 electricians in Charlotte, permits and job notes in spreadsheets.",
    "Sunrise Senior Care, 60 caregivers in Orlando, shift scheduling by phone.",
]
STEPS = ["classify", "pain_points", "score", "draft_email"]
isnum = lambda x: isinstance(x, (int, float)) and not isinstance(x, bool)
valid = {
    "classify": lambda o: isinstance(o, dict) and isinstance(o.get("industry"), str) and o["industry"] != "unknown" and isinstance(o.get("size"), str),
    "pain_points": lambda o: isinstance(o, list) and len(o) >= 1 and all(isinstance(x, str) for x in o),
    "score": lambda o: isinstance(o, dict) and isnum(o.get("score")) and 0 <= o["score"] <= 100,
    "draft_email": lambda o: isinstance(o, str) and len(o.strip()) > 20,
}
cases = []
steps_ok = steps_total = runs_done = inj = recovered = nat_fail = nat_rec = 0
for i, lead in enumerate(LEADS):
    fail_step = STEPS[(i >> 1) % 4] if i % 2 == 1 else None  # 8 of 16 runs get an injected first-attempt failure
    r = call("POST", "/api/runs", {"lead": lead, **({"fail_step": fail_step} if fail_step else {})})
    run = r["j"]; per = {}
    for s in STEPS:
        steps_total += 1
        per[s] = bool(run.get("out")) and valid[s](run["out"].get(s))
        steps_ok += per[s]
    log = run.get("log") or []
    complete = run.get("status") == "complete"
    runs_done += complete
    if fail_step:
        inj += 1
        if complete and any(l["step"] == fail_step and not l["ok"] for l in log) and any(l["step"] == fail_step and l["ok"] for l in log):
            recovered += 1
    for s in STEPS:
        fails = [l for l in log if l["step"] == s and not l["ok"] and not re.search("injected", l.get("error", ""))]
        if fails:
            nat_fail += 1
            if any(l["step"] == s and l["ok"] for l in log):
                nat_rec += 1
    cases.append({"lead": lead, "fail_step": fail_step, "http": r["status"], "status": run.get("status"), "valid_steps": per, "attempts": len(log), "ms": r["ms"]})
    print(i + 1, run.get("status"), json.dumps(per), f"{r['ms']}ms", flush=True)
metrics = {
    "step_completion": {**ratio(steps_ok, steps_total), "definition": "steps whose output passed a schema check (industry set, >=1 pain point, score 0-100, email >20 chars)"},
    "run_completion": ratio(runs_done, len(LEADS)),
    "recovery_rate": {**ratio(recovered, inj), "definition": "runs with an injected first-attempt failure that retried and completed"},
    "natural_step_failures": {"failed": nat_fail, "recovered": nat_rec},
}
out = {"project": NAME, "endpoint": BASE, "model": "@cf/meta/llama-3.3-70b-instruct-fp8-fast", "run_at": datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z"),
       "latency_ms": {"n": len(lat), "p50": pct(lat, 50), "p95": pct(lat, 95), "note": "client-side wall clock per HTTP request, box to Cloudflare edge"}, "metrics": metrics, "cases": cases}
(here / "results.json").write_text(json.dumps(out, indent=2) + "\n")
print(json.dumps({"metrics": metrics, "latency_ms": out["latency_ms"]}, indent=2))
