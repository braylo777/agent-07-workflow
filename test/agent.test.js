import { test } from "node:test";
import assert from "node:assert/strict";
import { execute, newRun, STEPS } from "../src/workflow.js";

const kv = () => { const m = new Map(); return { m, get: async (k) => (m.has(k) ? JSON.parse(m.get(k)) : null), put: async (k, v) => m.set(k, v) }; };
const ai = (onCall = () => {}) => ({ run: async (_m, inp) => { const s = inp.messages[0].content; onCall(s);
  if (s.startsWith("Classify")) return { response: '{"industry":"plumbing","size":"small"}' };
  if (s.startsWith("List")) return { response: '["missed calls","scheduling","quotes"]' };
  if (s.startsWith("Score")) return { response: '{"score":82,"reason":"high call volume"}' };
  return { response: "Hi [Name], ..." }; } });

test("runs all steps and checkpoints", async () => {
  const env = { KV: kv(), AI: ai() };
  const r = await execute(env, newRun({ lead: "Joe's Plumbing, Austin" }), { backoff: 1 });
  assert.equal(r.status, "complete"); assert.equal(r.out.score.score, 82); assert.equal(r.log.length, STEPS.length);
  assert.equal(env.KV.m.size, 1);
});
test("injected failure is retried", async () => {
  const r = await execute({ KV: kv(), AI: ai() }, newRun({ lead: "x", fail_step: "score" }), { backoff: 1 });
  assert.equal(r.status, "complete"); assert.deepEqual(r.log.filter((l) => l.step === "score").map((l) => l.ok), [false, true]);
});
test("resume skips completed steps", async () => {
  const calls = []; const run = newRun({ lead: "x" }); run.out = { classify: { industry: "a" }, pain_points: ["p"] };
  await execute({ KV: kv(), AI: ai((s) => calls.push(s.split(" ")[0])) }, run, { backoff: 1 });
  assert.deepEqual(calls, ["Score", "Write"]);
});
