# flare-top metric mapping

Maintainer reference for the fields on screen. Source of truth:
`flare-helios` `GET /metrics` (Prometheus text) in
`NVIDIA/flare-local-inference`. The checked-out contract used to build this
tool is commit `bce032c`; the live smoke test ran against a `9a45b0b` server.

Upstream `bce032c` removed the separate proposal graph counters and reduced
the graph-launch kinds to nine; `flare-top` reads absent families as absent, so
both contracts display. The proposal fields read `—` on newer servers.

`/metrics?format=json` exposes a smaller `Snapshot`; the dashboard uses the
text exposition because it carries the labeled families (graph kinds, HTTP
APIs, speculative batch/width, host-store outcomes) and the host-cache
histograms.

## Windowing

- One scrape per poll interval (default 1s). A ring retains 150 samples.
- A rate is a counter delta over the selected 5s, 15s, or 60s trailing window.
  The first sample inside the window is the base; rate = delta / elapsed.
- A reset (`new < old`) counts as rising from zero for that scrape pair.
- A series absent from either endpoint of a pair contributes no delta, so a
  new label set never fabricates a spike.
- The graphs always use the 60s window, independent of the 1/2/3 focus.
- Family rates use parse-time family sums. Labeled series are summed only for
  instantaneous panel totals and ratios.

## Graphs and peaks

`v` / `V` cycle five graph pages:

| Page | Graphs |
|---|---|
| Rates | prefill, generation + single stream, GPU replay, host I/O |
| Speculation | acceptance, depth, verify steps/s, readiness wait |
| Engram | gather/s, disk bytes/s, disk-wait share, row hit |
| Requests | active, queue, completed/s, failed/s |
| Cache | prefix hit, host hit, KV used, reused tokens/s |

Each title carries the focused-window value and the session maximum. At tight
heights four graphs share a row and the titles abbreviate that pair as
`now pk peak`, for example `PREF 258 pk 1.5k`. The footer repeats peak prefill,
generation, single-stream, GPU, acceptance, and depth. Peaks are updated on
every scrape and survive window changes for the life of the process.

## Panels

| Field | Metric | Notes |
|---|---|---|
| `PREFILL` | `flare_prefill_tokens_total` | Prompt tokens consumed by completed prefill, excluding padding and prefix reuse |
| `GENERATE` | `flare_generated_tokens_total` | Committed generated tokens, including EOS and later-withheld stop tokens |
| `SINGLE` | generation rate ÷ time-weighted mean `flare_active_requests` | Per-live-slot pace over the focused window; `—` when no slots were live |
| `WEIGHTS` | verification rate × `flare_decode_weight_bytes` | Decode weight bandwidth when the package declares the bytes; otherwise the cell shows `LAST MIN` instead |
| `GPU BUSY` / `GPU REPLAY` | `flare_replay_gpu_nanoseconds_total` | Sum of maximum rank-local CUDA durations of completed graphs per wall second, including embedded readiness waits; a replay density, not an SM utilization sample |
| `LAST MIN` | `flare_generated_tokens_last_minute` | Server-side gauge, sampled every five seconds |
| `ACCEPT` | `flare_accepted_draft_tokens_total` ÷ `flare_draft_request_rounds_total` | Accepted drafts per draft-offering round; excludes the bonus token |
| `DEPTH` | `flare_verification_draft_tokens_total` ÷ `flare_verification_request_rounds_total` | Allocated draft positions per progressing decode/verification round, rejected drafts included |
| `VERIFY` | `flare_speculative_rounds_total` | Summed over every `{batch,width}` label set |
| `ACCEPTED` | `flare_accepted_draft_tokens_total` | Drafts/s |
| `SPEC GPU` | `flare_speculative_gpu_nanoseconds_total` ÷ `flare_speculative_rounds_total` | Cumulative milliseconds per completed combined graph |
| `EMITTED` / `OFFERED` | `flare_speculative_{emitted_tokens,offered_drafts}_total` | Per completed speculative graph |
| `PROPOSAL` | `flare_proposal_rounds_total` | Separate proposal graphs per second |
| `HISTORY` | `flare_history_drafts_enabled` | Gauge: 1 / 0 |
| `CONSTRAIN` | `flare_constrained_max_inflight` | Maximum outstanding executor frames after a constrained submission |
| `MIXED` | `flare_mixed_grammar_decode_steps_total` | Decode submissions mixing grammar-constrained and unconstrained lanes |

### Requests and health

| Field | Metric |
|---|---|
| `ACTIVE`, `QUEUE` | `flare_active_requests`, `flare_pending_requests` |
| `SLOTS` | `flare_free_request_slots`, plus active/retiring for the total |
| `ADMITTED/S` | `flare_requests_admitted_total` |
| `COMPLETE/S` | `flare_requests_completed_total` |
| `FAIL/S` | `flare_requests_failed_total` |
| `CANCEL/S` | `flare_requests_cancelled_total` |
| `PREEMPT/S` | `flare_preemptions_total` |
| `INGRESS` | `flare_http_inflight_requests` / `flare_http_request_capacity` |
| `FRONTEND` | `flare_frontend_active_workers` / `flare_frontend_worker_capacity` |
| `READY WAIT` | `flare_replay_readiness_wait_nanoseconds_total` ÷ `flare_replay_readiness_samples_total` |
| `HTTP` | `flare_http_requests_total`, summed over `{api}` |
| `SCHED` | `flare_scheduler_executions_total`, summed over `{reason}` |
| `UP` (header) | `flare_uptime_seconds` |

### KV and checkpoints

- `flare_cached_kv_tokens`, `flare_free_kv_tokens`, `flare_total_kv_tokens`
  are logical token capacities. The fallback when the token gauges are absent
  is `pages × flare_kv_page_tokens`.
- `KV USED` = `(total_kv_tokens - free_kv_tokens) / total_kv_tokens`.
- `CACHED %` = `cached_kv_tokens / total_kv_tokens`, the share retained by
  immutable checkpoints.
- `SNAPSHOT` = `(snapshot_capacity - free_snapshots) / snapshot_capacity`.
- `BACKING` = `flare_snapshot_backing_pages`.
- `CHECKPOINT` = `flare_cached_checkpoints`.
- `PUBLISHED` = `flare_snapshot_publications_total`.
- `PREEMPT` = `flare_preemptions_total`.

### Prefix cache and host store

| Field | Metric |
|---|---|
| `GPU HIT` | `flare_cache_hits_total` ÷ `flare_cache_lookups_total` |
| `REUSED` | `flare_cache_reused_tokens_total` |
| `HOST HIT` | `flare_host_cache_hits_total` ÷ `flare_host_cache_lookups_total` |
| `HOST OUT` / `IN` | `flare_host_cache_bytes_{out,in}_total` |
| `HOST MEM` / `DISK` | `flare_host_cache_{memory,disk}_{records,bytes}` |
| `HOST LOOKUP` | `flare_host_cache_lookups_total` |
| `HOST HITS` | `flare_host_cache_hits_total` (cumulative count) |
| `STORE/FETCH` | `flare_host_cache_{stored,fetched}_total` |
| `RETRY` | `flare_host_cache_retry_{success,abandoned}_total` |
| `FAILURES` | `flare_host_cache_{store,fetch}_failed_total` |
| `OUTCOMES` | `admitted` / `no_match` / the sum of `cost_rejected`, `capacity_rejected`, `enqueue_rejected` |

The exposition also carries three host-cache histograms:
`flare_host_cache_missing_tokens`, `flare_host_cache_estimated_transfer_ns`,
and `flare_host_cache_estimated_prefill_ns`. The parser retains their buckets,
sum, and count; the dashboard currently shows the counters and rates, not the
histogram quantiles.

### Engram

| Field | Metric |
|---|---|
| `ENG GATHER` | `flare_engram_gathers_total` and `flare_engram_gather_nanoseconds_total` |
| `ENG DISK` | `flare_engram_disk_bytes_total`; wait share = `flare_engram_disk_wait_nanoseconds_total` ÷ gather wall time |
| `ENG CACHE` | `flare_engram_row_cache_{resident,capacity}_rows` |
| `ROW HIT` | `flare_engram_row_cache_{hits,misses}_total` |

### Input cache

`flare_input_decoded_images_total`, `flare_input_cache_metadata_*`,
`flare_input_cache_payload_*`, and
`flare_input_cache_{memory,disk}_{records,bytes}` are summarized as
`INPUT hit % · memory records / disk records`. These are server-level metrics,
not part of the JSON snapshot.

## Ratios vs. cumulative values

The UI computes windowed ratios for the focused window:

- GPU hit, host hit, acceptance, depth, readiness wait per graph, Engram row
  hit, and Engram disk-wait share.
- A ratio with no denominator traffic in the window reads `—`, never a
  fabricated 0% or a stale cumulative ratio.
- Counters that have no windowed display (for example `GRAPH SPEC launches`)
  are labeled as cumulative launch counts.

## Stall definition

For each measured interval in the trailing 60s:

```
generated_rate < 0.25 × median(nonzero generated_rate samples)
AND prefill_rate > 0
AND active_requests > 0
```

Requires at least five measured intervals and a positive median. Red dots on
every graph mark flagged intervals; the footer folds them into the selected
window. This detects throughput collapse, not a specific scheduler error.

## Deliberately not shown

| vLLM/sglang field | Reason |
|---|---|
| TTFT / ITL / E2E quantiles | FLARE exposes no latency histograms |
| Queue time | No request-timing histogram |
| Per-tokenizer CPU | No per-tokenizer split |
| Prefix-cache hit *tokens* by source | FLARE reports reused tokens, not a per-source counter |
| vLLM `kv_cache_usage_perc` | Reconstructed from FLARE's page/token gauges instead |
