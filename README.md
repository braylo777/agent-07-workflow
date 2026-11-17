# agent-07-workflow

**Agent Workflow: durable, checkpointed AI pipeline**

**Live demo:** https://agent-07-workflow.braylo777.workers.dev

![Demo screenshot](docs/screenshot.png)

## What it is
A multi-step AI workflow engine (project 7 of a 10-part agentic AI portfolio). A sales lead goes through four LLM steps (classify, find pain points, score, draft an email). Every step is checkpointed to KV, failed steps retry with exponential backoff, and a run can be resumed from its last completed step.

## Architecture
```mermaid
flowchart LR
  U[POST /api/runs] --> RL{rate limit}
  RL --> C[classify]
  C --> P[pain_points] --> S[score] --> E[draft_email] --> Done[(status: complete)]
  C & P & S & E -. checkpoint .-> KV[(Workers KV)]
  S -- error --> B[retry w/ backoff<br/>max 3 attempts] --> S
  Resume[POST /api/runs/:id/resume] --> KV
```

## How it works
- `src/agent07/workflow.py` and `worker/src/workflow.rs`: step definitions, retry loop (`MAX_ATTEMPTS = 3`, backoff 250ms × 2ⁿ), KV checkpoint after every step, resume skips completed steps.
- `worker/src/workflow.rs` (router) and `worker/src/worker_entry.rs`: REST API (`/api/runs`, `/api/runs/:id`, `/api/runs/:id/resume`, `/api/steps`), input validation, per-IP rate limit.
- `fail_step` lets you inject a first-attempt failure into any step to watch recovery live.
- Runs serverless on Cloudflare Workers with the free Workers AI tier (`@cf/meta/llama-3.3-70b-instruct-fp8-fast`); 5 requests/min per IP keeps the demo free.

## Eval results
Measured against the **live** deployment with real model calls (`python3 evals/run.py`), run 2026-10-27 19:23 CT. Raw per-case output: [`evals/results.json`](evals/results.json). Small sample sizes, kept small to stay inside the free daily model quota, so read these as indicative, not definitive.

| Metric | Result |
|---|---|
| Step completion (schema-valid step outputs) | 64/64 (100%) |
| Run completion | 16/16 (100%) |
| Recovery rate (injected failures that retried and completed) | 8/8 (100%) |
| Natural (non-injected) step failures | 0 failed, 0 recovered |
| Latency p50 / p95 (end-to-end HTTP) | 5237 ms / 8768 ms (n=16) |

## Tests
Python: 10 tests pass; Rust: 5 test functions pass (the LLM and KV are mocked, so they run offline). Six scripted runs (normal, injected failure, garbage output, missing field, total failure, then resume) give identical outputs and logs for the old JavaScript, the Python and the Rust code.

## Run it
```bash
python3 -m pytest                      # Python tests
cd worker && cargo test                # Rust tests
cd worker && worker-build --release    # build the Worker (wasm)
BASE=http://localhost:8787 python3 evals/run.py   # live evals
wrangler dev                           # local; Workers AI via your Cloudflare login
wrangler deploy                        # free tier, no paid API keys
```

The previous JavaScript version is on the `legacy-js` branch and the `pre-pycrust` tag.

## License
MIT
