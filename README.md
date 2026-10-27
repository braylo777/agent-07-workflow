# agent-07-workflow

A **durable agent workflow**: a multi-step LLM pipeline with checkpointing, retries with exponential backoff, and resumable runs. It runs serverless on Cloudflare Workers with the free Workers AI tier and KV. Project 7 of a 10-part agentic AI portfolio.

**Live demo:** https://agent-07-workflow.braylo777.workers.dev

## What it is
Submit a sales lead. The workflow runs four steps (`classify` → `pain_points` → `score` → `draft_email`), saving each result as it completes. Look up any run at `GET /api/runs/:id` and resume a failed one with `POST /api/runs/:id/resume`. Pass `fail_step` to inject a failure and watch the retry in the run log.

## How it works
```
POST /api/runs ─▶ run{id, input, out:{}, log:[]}
   for step in [classify, pain_points, score, draft_email]:
       skip if checkpointed
       try step (LLM, structured JSON) ─ fail ─▶ retry ×3 (250ms, 500ms…)
       checkpoint run ─▶ KV
   status: complete | failed (resumable)
```
- `src/workflow.js`: the step definitions and a small workflow engine (checkpoint, retry, resume, per-attempt log with timings).
- Each step only sees the outputs it needs from earlier steps, and parses structured JSON with safe fallbacks.
- Runs expire after 3 days. Guardrails: 5 requests/min per IP, 500-char cap.
- For long-running production jobs, the same step model maps directly onto Cloudflare Workflows (`step.do`).

## Run it
```bash
npm install
npm test            # offline unit tests (Node 20+), LLM is mocked
npx wrangler dev    # local; Workers AI runs via your Cloudflare login
npx wrangler deploy # free tier, no paid API keys
```
`wrangler.toml` needs your KV namespace id first: `npx wrangler kv namespace create KV`.

## Stack
Cloudflare Workers · Workers AI (`@cf/meta/llama-3.3-70b-instruct-fp8-fast`, Workers KV) · vanilla JS · node:test

## License
MIT
