# Performance — Gate G3 (1.0)

Env: Linux x86_64, AMD Ryzen 9 7900 (24 threads, 30 GB), rustc 1.98; `owasidea` **debug** binary; Shipyard @ `release/1.0` 6b8589a; temp `OWASIDEA_DATA_DIR`, ports 18941-18943.
Method: spawn host → `GET /health` 200 (1 cache-filling run + 8 warm runs); 50 sequential keep-alive `POST /plugin/{name}/call` after 3 warm-ups; `build`/`bundle` with no source change.
- Host startup, warm Wasmtime cache: p50 114 ms, p95 117 ms (budget ≤300 ms) — pass
- `core-rust.ping` (small call): p50 0.9 ms, p95 1.1 ms (budget p95 ≤20 ms) — pass
- `changelog-ts.render` (TS component): p50 1.7 ms, p95 1.8 ms (budget p95 ≤100 ms) — pass
- `owasidea build` incremental (first rerun 34.9 s): 8.9 s (budget ≤60 s) — pass
- `owasidea bundle --bundles deb`: 23.6 s (budget ≤5 min); deb 25.4 MB, `dist/` 52 MB — pass
Limits: cold start measured once only (3.55 s, empty cache), not a distribution; Windows/macOS unmeasured; calls are sequential (no load test); debug host binary is pessimistic for host code; bundle reused a warm release target dir, so a cold first bundle is slower.
