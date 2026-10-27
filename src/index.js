import { json, limited, body, cap } from "./core.js";
import { execute, getRun, newRun, STEPS } from "./workflow.js";
import page from "./page.html";

export default {
  async fetch(req, env) {
    const url = new URL(req.url);
    const m = url.pathname.match(/^\/api\/runs\/([a-z0-9-]{4,40})(\/resume)?$/);
    if (req.method === "GET" && url.pathname === "/api/steps") return json(STEPS.map((s) => s.name));
    if (req.method === "GET" && m && !m[2]) { const r = await getRun(env, m[1]); return r ? json(r) : json({ error: "not found" }, 404); }
    if (req.method === "POST" && (url.pathname === "/api/runs" || (m && m[2]))) {
      if (await limited(req, env)) return json({ error: "Rate limit: try again in a minute." }, 429);
      let run;
      if (m) { run = await getRun(env, m[1]); if (!run) return json({ error: "not found" }, 404); if (run.status === "complete") return json(run); }
      else {
        const b = await body(req);
        if (!cap(b.lead)) return json({ error: "lead required (<=500 chars)" }, 400);
        if (b.fail_step && !STEPS.some((s) => s.name === b.fail_step)) return json({ error: "unknown fail_step" }, 400);
        run = newRun({ lead: b.lead, fail_step: b.fail_step });
      }
      try { return json(await execute(env, run)); } catch (e) { return json({ error: "Workflow error: " + e.message, id: run.id }, 502); }
    }
    return new Response(page, { headers: { "content-type": "text/html;charset=utf-8" } });
  },
};
