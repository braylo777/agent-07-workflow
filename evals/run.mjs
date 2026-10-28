// Live eval suite: hits the deployed Worker (real Workers AI calls). Run: node evals/run.mjs
const NAME = "agent-07-workflow";
const BASE = process.env.BASE || "https://"+NAME+".braylo777.workers.dev";
const GAP = Number(process.env.GAP_MS || 13000); // 5 req/min rate limit on the demo
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let last = 0; const lat = [];
async function call(method, path, body, { timed = true } = {}) {
  const w = last + GAP - Date.now(); if (w > 0) await sleep(w);
  for (let i = 0; i < 3; i++) {
    last = Date.now(); const t0 = Date.now();
    const r = await fetch(BASE + path, method === "GET" ? {} : { method, headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
    const ms = Date.now() - t0; const j = await r.json().catch(() => ({}));
    if (r.status === 429) { await sleep(61000); continue; }
    if (timed) lat.push(ms); return { status: r.status, ms, j };
  }
  throw new Error("rate limited 3x");
}
const pct = (a, p) => { if (!a.length) return null; const s = [...a].sort((x, y) => x - y); return s[Math.max(0, Math.ceil((p / 100) * s.length) - 1)]; };
const ratio = (n, d) => ({ passed: n, total: d, rate: d ? n / d : null });
async function save(metrics, cases) {
  const fs = await import("node:fs");
  const out = { project: NAME, endpoint: BASE, model: "@cf/meta/llama-3.3-70b-instruct-fp8-fast", run_at: new Date().toISOString(),
    latency_ms: { n: lat.length, p50: pct(lat, 50), p95: pct(lat, 95), note: "client-side wall clock per HTTP request, box to Cloudflare edge" }, metrics, cases };
  fs.mkdirSync(new URL(".", import.meta.url), { recursive: true });
  fs.writeFileSync(new URL("./results.json", import.meta.url), JSON.stringify(out, null, 2) + "\n");
  console.log(JSON.stringify({ metrics, latency_ms: out.latency_ms }, null, 2));
}
const LEADS = [
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
];
const STEPS = ["classify", "pain_points", "score", "draft_email"];
const valid = {
  classify: (o) => o && typeof o.industry === "string" && o.industry !== "unknown" && typeof o.size === "string",
  pain_points: (o) => Array.isArray(o) && o.length >= 1 && o.every((x) => typeof x === "string"),
  score: (o) => o && typeof o.score === "number" && o.score >= 0 && o.score <= 100,
  draft_email: (o) => typeof o === "string" && o.trim().length > 20,
};
const cases = []; let stepsOk = 0, stepsTotal = 0, runsDone = 0, inj = 0, recovered = 0, natFail = 0, natRec = 0;
for (const [i, lead] of LEADS.entries()) {
  const fail_step = i % 2 === 1 ? STEPS[(i >> 1) % 4] : undefined; // 8 of 16 runs get an injected first-attempt failure
  const r = await call("POST", "/api/runs", fail_step ? { lead, fail_step } : { lead });
  const run = r.j; const per = {};
  for (const s of STEPS) { stepsTotal++; per[s] = !!(run.out && valid[s](run.out[s])); if (per[s]) stepsOk++; }
  const complete = run.status === "complete"; if (complete) runsDone++;
  if (fail_step) { inj++; if (complete && run.log.some((l) => l.step === fail_step && !l.ok) && run.log.some((l) => l.step === fail_step && l.ok)) recovered++; }
  for (const s of STEPS) { const fails = (run.log || []).filter((l) => l.step === s && !l.ok && !/injected/.test(l.error)); if (fails.length) { natFail++; if ((run.log || []).some((l) => l.step === s && l.ok)) natRec++; } }
  cases.push({ lead, fail_step: fail_step || null, http: r.status, status: run.status, valid_steps: per, attempts: (run.log || []).length, ms: r.ms });
  console.error(i + 1, run.status, JSON.stringify(per), r.ms + "ms");
}
await save({
  step_completion: { ...ratio(stepsOk, stepsTotal), definition: "steps whose output passed a schema check (industry set, >=1 pain point, score 0-100, email >20 chars)" },
  run_completion: ratio(runsDone, LEADS.length),
  recovery_rate: { ...ratio(recovered, inj), definition: "runs with an injected first-attempt failure that retried and completed" },
  natural_step_failures: { failed: natFail, recovered: natRec },
}, cases);
process.exit(0);
