# ticker

Stateful service component. The host keeps its instance alive between
`start` and `stop`.

- `POST /plugin/ticker/service/start` starts it (repeat-safe).
- `GET  /plugin/ticker/service/status` reports `running`.
- `POST /plugin/ticker/service/stop` drops the instance.
- `POST /plugin/ticker/reload` is refused with HTTP 409 while it runs.
