# flare-top

Live terminal dashboard for a [FLARE](https://github.com/NVIDIA/flare-local-inference)
(Helios) inference server: prefill/generation rates, request and slot health,
KV/checkpoint pools, speculative decoding, host prefix cache, and Engram
telemetry. One Rust binary, no daemon, no storage.

Inspired by [sgtop](https://github.com/mratsim/sgtop) and
[vllm-top](https://github.com/mratsim/vllm-top), which do the same for sglang
and vLLM. The layout and interaction model follow those tools.

## Quick start

```sh
cargo install --path .
flare-top --url http://127.0.0.1:5991
flare-top --url http://localhost:8000 --interval 0.5
flare-top --url https://gpu-box.example.org --insecure --api-key sk-...
flare-top --once                     # one text snapshot for scripts
```

## Keys

| Key | Action |
|---|---|
| `q` / `Esc` / `Ctrl-C` | quit |
| `space` | pause scraping |
| `c` | toggle compact / full layout |
| `g` | toggle graphs |
| `v` / `V` | next / previous graph page |
| `1` `2` `3` | focus the 5s / 15s / 60s window |
| `t` | cycle theme |
| `e` | explain the dashboard |
| `?` | keymap |
| `+` / `-` | poll interval |
| `↑` / `↓` | scroll the explain screen |

## Notes

- Scrapes `GET /metrics` from one FLARE rank-zero server.
- FLARE exposes no TTFT/ITL histograms, so the dashboard shows the counters it
  actually provides.
- Five graph pages (`v`): rates, speculation, Engram, requests, cache. Every
  graph title shows the current value and the session peak (`pk` in compact
  rows); the footer repeats peak prefill/decode/single/GPU/accept/depth.
- Exact field-to-metric mapping: [`docs/metrics.md`](docs/metrics.md).
- `--insecure` accepts any TLS certificate: the bearer token and metrics
  transit unverified.

## Build

```sh
cargo build --release        # target/release/flare-top
cargo test
```

## License

GPL-3.0. See [`LICENSE`](LICENSE).
