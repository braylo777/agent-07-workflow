import { test } from "node:test";
import assert from "node:assert/strict";
import { execute, newRun, getRun, STEPS, MAX_ATTEMPTS } from "../src/workflow.js";
import { parseJSON, cap } from "../src/core.js";

const kv = () => { const m = new Map(); return { m, get: async (k) => (m.has(k) ? JSON.parse(m.get(k)) : null), put: async (k, v) => m.set(k, v) }; };
const okAI = { run: async (_m, i) => { const s = i.messages[0].content;
  if (s.startsWith("Classify")) return { response: 'Sure: {"industry":"hvac","size":"small"}' };
  if (s.startsWith("List")) return { response: '["a","b","c"]' };
  if (s.startsWith("Score")) return { response: '{"score":70,"reason":"r"}' };
  return { response: "Hi [Name]" }; } };

test("step order is fixed", () => assert.deepEqual(STEPS.map((s) => s.name), ["classify", "pain_points", "score", "draft_email"]));
test("JSON is extracted from chatty model output", async () => {
  const r = await execute({ KV: kv(), AI: okAI }, newRun({ lead: "x" }), { backoff: 1 }); assert.equal(r.out.classify.industry, "hvac");
});
test("unparseable output falls back instead of crashing", async () => {
  const r = await execute({ KV: kv(), AI: { run: async () => ({ response: "no json here" }) } }, newRun({ lead: "x" }), { backoff: 1 });
  assert.equal(r.status, "complete"); assert.equal(r.out.score.reason, "unparseable"); assert.deepEqual(r.out.pain_points, []);
});
test("a step failing every attempt marks the run failed after MAX_ATTEMPTS", async () => {
  const ai = { run: async (_m, i) => { if (i.messages[0].content.startsWith("Score")) throw new Error("boom"); return okAI.run(_m, i); } };
  const r = await execute({ KV: kv(), AI: ai }, newRun({ lead: "x" }), { backoff: 1 });
  assert.equal(r.status, "failed"); assert.equal(r.log.filter((l) => l.step === "score").length, MAX_ATTEMPTS); assert.equal(r.out.draft_email, undefined);
});
test("failed run resumes and completes once the fault clears", async () => {
  const env = { KV: kv(), AI: null }; let broken = true;
  env.AI = { run: async (_m, i) => { if (broken && i.messages[0].content.startsWith("Score")) throw new Error("down"); return okAI.run(_m, i); } };
  const run = await execute(env, newRun({ lead: "x" }), { backoff: 1 }); assert.equal(run.status, "failed");
  broken = false; const saved = await getRun(env, run.id); const r2 = await execute(env, saved, { backoff: 1 });
  assert.equal(r2.status, "complete"); assert.equal(r2.log.filter((l) => l.step === "classify").length, 1);
});
test("each run is persisted under its id", async () => {
  const env = { KV: kv(), AI: okAI }; const r = await execute(env, newRun({ lead: "x" }), { backoff: 1 });
  assert.equal((await getRun(env, r.id)).status, "complete");
});
test("helpers", () => { assert.equal(parseJSON("nope", 1), 1); assert.ok(cap("hi")); assert.ok(!cap("")); assert.ok(!cap("x".repeat(501))); });
