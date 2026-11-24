# gpui-counter

Native UI reference app: a GPUI window calls a Rust library plugin through the
host's loopback HTTP API.

```sh
owasidea build
owasidea run
```

The window starts with `OWASIDEA_HOST` set by the CLI. Without a display, use
`owasidea build` and `owasidea run --no-native-ui`; the host and plugin still
build and run headlessly.

GPUI is pinned to `=0.2.2` in `plugins/window/Cargo.toml`; the example has its
own workspace and lockfile so the framework workspace is untouched.
