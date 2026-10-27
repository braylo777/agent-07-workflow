import { ask, parseJSON } from "./core.js";

// A durable, checkpointed workflow engine: each step's output is saved to KV, failed steps
// retry with exponential backoff, and a run can be resumed from its last completed step.
export const STEPS = [
  { name: "classify", run: (env, s) => ask(env, "Classify this business lead. Reply ONLY JSON: {\"industry\":string,\"size\":\"solo|small|mid|large\"}", s.input.lead, { max_tokens: 80 }).then((r) => parseJSON(r, { industry: "unknown", size: "unknown" })) },
  { name: "pain_points", run: (env, s) => ask(env, "List the top 3 likely operational pain points AI could solve for this business. Reply ONLY a JSON array of strings.", `${s.input.lead}\nIndustry: ${s.out.classify.industry}`, { max_tokens: 200 }).then((r) => parseJSON(r, [])) },
  { name: "score", run: (env, s) => ask(env, "Score this lead 0-100 for fit with an AI automation agency. Reply ONLY JSON: {\"score\":number,\"reason\":string}", JSON.stringify({ lead: s.input.lead, ...s.out }), { max_tokens: 120 }).then((r) => parseJSON(r, { score: 0, reason: "unparseable" })) },
  { name: "draft_email", run: (env, s) => ask(env, "Write a short, friendly first-touch email (max 90 words) offering to solve the pain points. No placeholders except [Name].", JSON.stringify({ lead: s.input.lead, pains: s.out.pain_points }), { max_tokens: 250 }) },
];
export const MAX_ATTEMPTS = 3;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function execute(env, run, { backoff = 250 } = {}) {
  run.status = "running";
  for (const step of STEPS) {
    if (run.out[step.name] !== undefined) continue; // checkpoint: already done
    for (let attempt = 1; ; attempt++) {
      const t0 = Date.now();
      try {
        if (run.input.fail_step === step.name && attempt === 1) throw new Error("injected failure (demo)");
        run.out[step.name] = await step.run(env, run);
        run.log.push({ step: step.name, attempt, ok: true, ms: Date.now() - t0 });
        break;
      } catch (e) {
        run.log.push({ step: step.name, attempt, ok: false, error: e.message, ms: Date.now() - t0 });
        if (attempt >= MAX_ATTEMPTS) { run.status = "failed"; await persist(env, run); return run; }
        await sleep(backoff * 2 ** (attempt - 1));
      }
    }
    await persist(env, run); // checkpoint after every step
  }
  run.status = "complete"; await persist(env, run); return run;
}
export async function persist(env, run) { run.updated = new Date().toISOString(); await env.KV.put(`run:${run.id}`, JSON.stringify(run), { expirationTtl: 86400 * 3 }); }
export async function getRun(env, id) { return env.KV.get(`run:${id}`, "json"); }
export function newRun(input) { return { id: crypto.randomUUID().slice(0, 8), input, out: {}, log: [], status: "queued", created: new Date().toISOString() }; }
