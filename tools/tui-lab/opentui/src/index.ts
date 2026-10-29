import {
  BoxRenderable,
  TextRenderable,
  createCliRenderer,
} from "@opentui/core"

const tabs = ["Overview", "Plugins", "Logs", "Metrics", "Tests"]

const logs = [
  "12:04:01 INFO  host ready on 127.0.0.1:8787",
  "12:04:02 INFO  loaded core-rust (wasmtime 50)",
  "12:04:03 WARN  audit-go: rebuild failed, keeping previous component",
  "12:04:03 ERROR build: exit 1; last output: go: no such tool componentize-go",
  "12:04:04 INFO  service watchtower-rust started",
]

const views: Record<string, string[]> = {
  Overview: [
    "App      hello",
    "Plugins  4 loaded, 1 failing",
    "Services 1 running",
    "Build    128 · 8.9s",
    "",
    "Next steps",
    "  fix audit-go build",
    "  press r to retry",
    "",
    "Latest logs",
    ...logs,
  ],
  Plugins: [
    "● core-rust          library   loaded",
    "✕ audit-go           library   error",
    "● watchtower-rust    service   loaded",
    "● board-svelte       ui        loaded",
  ],
  Logs: logs,
  Metrics: [
    "core-rust p95 latency   1.1 ms / 20 ms   ███░░░░░░░░░░░░░░░░░",
    "host memory            42 MB / 256 MB    ██░░░░░░░░░░░░░░░░░░",
    "",
    "requests  1 284   errors 3   fuel 12%",
    "calls/s   41.2    p50 0.9 ms   p95 1.1 ms",
    "top       core-rust 72% · changelog-ts 18% · audit-go 10%",
  ],
  Tests: [
    "PASS smoke:greet         3 ms",
    "PASS smoke:sum           2 ms",
    "FAIL ui:round-trip       timeout 30s",
  ],
}

const renderer = await createCliRenderer({ exitOnCtrlC: true })

const root = new BoxRenderable(renderer, {
  id: "root",
  flexDirection: "column",
  width: "100%",
  height: "100%",
  padding: 1,
  gap: 1,
})
renderer.root.add(root)

const header = new BoxRenderable(renderer, {
  id: "header",
  border: true,
  borderColor: "#22d3ee",
  title: "Owasidea Dev",
  height: 3,
  paddingX: 1,
})
root.add(header)
header.add(new TextRenderable(renderer, { content: "build #128  ● host running  ● 4 plugins  ● 1 service" }))

const errorBox = new BoxRenderable(renderer, {
  id: "error",
  backgroundColor: "#991b1b",
  height: 3,
  paddingX: 1,
})
root.add(errorBox)
errorBox.add(
  new TextRenderable(renderer, {
    content: "BUILD FAILED  plugins/audit-go: componentize-go failed (exit 1) — see Logs",
  }),
)

const tabBar = new TextRenderable(renderer, { id: "tabs", content: "" })
root.add(tabBar)

const body = new BoxRenderable(renderer, {
  id: "body",
  border: true,
  borderColor: "#475569",
  title: "Overview",
  flexGrow: 1,
  padding: 1,
})
root.add(body)

const bodyText = new TextRenderable(renderer, { id: "body-text", content: "" })
body.add(bodyText)

root.add(new TextRenderable(renderer, { content: "1-5 tabs  ←/→ move  r rebuild  q quit" }))

let tab = 0
let buildError = true

function render() {
  tabBar.content = tabs
    .map((name, i) => (i === tab ? `[ ${name} ]` : `  ${name}  `))
    .join("│")
  body.title = tabs[tab]
  bodyText.content = views[tabs[tab]].join("\n")
  errorBox.visible = buildError
}

renderer.keyInput.on("keypress", (key) => {
  if (key.name === "q" || (key.ctrl && key.name === "c")) {
    renderer.destroy()
    return
  }
  if (key.name === "r") {
    buildError = false
    render()
    return
  }
  if (/^[1-5]$/.test(key.name)) {
    tab = Number(key.name) - 1
    render()
    return
  }
  if (key.name === "left" && tab > 0) tab--
  if (key.name === "right" && tab < tabs.length - 1) tab++
  render()
})

render()
