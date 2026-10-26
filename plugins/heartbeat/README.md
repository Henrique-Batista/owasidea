# heartbeat service example

Minimal stateful Rust service plugin. `start-service` and `service-status` run
on one persistent Wasmtime instance; host stop drops that instance, and the next
start gets fresh guest state.

Build with `cargo component build --release` or `owasidea build`.
