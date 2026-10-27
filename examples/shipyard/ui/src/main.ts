import './styles.css';
import { attachBridge } from './bridge';

interface OwasideaRuntime {
  host?: string;
  app?: string;
  version?: string;
  build_id?: string | null;
}
declare global {
  interface Window {
    __OWASIDEA__?: OwasideaRuntime;
  }
}
declare const __OWASIDEA_HOST__: string | undefined;

function isLoopbackHost(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' && (url.hostname === '127.0.0.1' || url.hostname === 'localhost');
  } catch {
    return false;
  }
}

const runtimeHost = window.__OWASIDEA__?.host;
let resolvedHost = '';
if (runtimeHost && isLoopbackHost(runtimeHost)) {
  resolvedHost = runtimeHost;
} else if (typeof __OWASIDEA_HOST__ === 'string' && __OWASIDEA_HOST__ && isLoopbackHost(__OWASIDEA_HOST__)) {
  resolvedHost = __OWASIDEA_HOST__;
}
if (!resolvedHost) {
  throw new Error('Owasidea host is not configured. Run via `owasidea dev`/`owasidea run`, or set OWASIDEA_HOST at build time.');
}
const HOST = resolvedHost;
const OWASIDEA_BUILD_ID = window.__OWASIDEA__?.build_id ?? null;
const view = document.getElementById('view')!;
const hostPill = document.getElementById('host-pill')!;
const hostLabel = document.getElementById('host-label')!;
hostLabel.textContent = HOST;

type WasiPlugin = {
  name: string;
  version: string;
  language: string;
  type: string;
  runtime: string;
  exports: string[];
  imports_wasi?: number;
  imports_host?: string[];
  served_on?: string | null;
};
type UiPlugin = {
  name: string;
  version: string;
  language: string;
  entry: string;
  sha256: string;
  files: Record<string, string>;
  bridge?: string[];
};
type Overview = {
  plugins: number;
  libraries: number;
  executables: number;
  services: number;
  runtimes: string[];
};
type DevStatus = {
  active: boolean;
  hot_reload: boolean;
  watch_paths: string[];
  last_reload: Record<string, { ok: boolean; http_status: number; timestamp_unix: number }>;
};

type RunResult = {
  key: string;
  label: string;
  plugin: string;
  language: string;
  function: string;
  value?: unknown;
  error?: string;
  ms: number;
  why: string;
  runtime: string;
};

let plugins: WasiPlugin[] = [];
let uiPlugins: UiPlugin[] = [];
let logSource: EventSource | null = null;
let hostOnline = false;
let hostErrorShown = false;
let hostRetry = 0;

const pluginMeta: Record<string, { role: string; why: string; detail: string }> = {
  'core-rust': {
    role: 'Rust readiness engine',
    why: 'Rust becomes a WASI component. It scores changes and checks, persists the report through host KV, and writes a Markdown file through the reports/ preopen.',
    detail: 'Wasmtime · fresh instance per call · host KV + filesystem',
  },
  'pipeline-go': {
    role: 'Composed Go → Rust pipeline',
    why: 'The Go client imports the Rust readiness interface. wasm-tools compose links the Rust component into this component at build time, so one call crosses two languages.',
    detail: 'Wasmtime · build-time composition · host KV + filesystem',
  },
  'audit-go': {
    role: 'Go dependency audit',
    why: 'Go reads the dependency list, flags alpha/beta/rc/0.0.x versions, and emits a host-log line visible in the Logs tab.',
    detail: 'Wasmtime · host-log import',
  },
  'changelog-ts': {
    role: 'TypeScript changelog',
    why: 'TypeScript is compiled with jco/componentize-js. It turns raw change lines into categorized Markdown release notes.',
    detail: 'Wasmtime · jco component',
  },
  'policy-csharp': {
    role: 'C# policy gate',
    why: 'componentize-dotnet compiles C# into a component. It classifies risk and calculates the next semantic version.',
    detail: 'Wasmtime · .NET WASI component',
  },
  'fingerprint-c': {
    role: 'C release fingerprint',
    why: 'A wasi-sdk + wit-bindgen C component. It produces a deterministic FNV-1a fingerprint of the release input.',
    detail: 'Wasmtime · wasi-sdk C component',
  },
  'metrics-cpp': {
    role: 'C++ text metrics',
    why: 'A C++20 wasi-sdk component. It analyzes the generated changelog and returns word/line/reading-time metrics.',
    detail: 'Wasmtime · wasi-sdk C++ component',
  },
  'watchtower-rust': {
    role: 'Persistent Rust service',
    why: 'A stateful lifecycle plugin. start-service creates one persistent instance; status queries it; stop drops it.',
    detail: 'Wasmtime · persistent instance · /service/{start,status,stop}',
  },
  'webhook-go': {
    role: 'Go executable HTTP plugin',
    why: 'Exports wasi:http/incoming-handler. The host mounts it at /app/webhook-go and forwards real HTTP requests and responses.',
    detail: 'Wasmtime · wasi:http inbound · /app/webhook-go',
  },
  'board-svelte': {
    role: 'Svelte UI bundle',
    why: 'A separate Vite/Svelte build. It runs only in the webview, in a sandboxed iframe, after its asset hashes are verified, and calls the allowlisted changelog-ts.render through the parent bridge.',
    detail: 'webview only · js-wasm · bridge: changelog-ts.render',
  },
  'map-react': {
    role: 'React UI bundle',
    why: 'A separate Vite/React build. It visualizes the architecture and runs only in the webview, isolated from the dashboard.',
    detail: 'webview only · js-wasm · no bridge declared',
  },
};

const languages: Record<string, string> = {
  rust: 'Rust',
  go: 'Go',
  typescript: 'TypeScript',
  javascript: 'JavaScript',
  csharp: 'C#',
  c: 'C',
  cpp: 'C++',
  svelte: 'Svelte',
  react: 'React',
};

function escapeHtml(value: unknown): string {
  return String(value).replace(/[&<>"']/g, ch => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  })[ch]!);
}

function languageName(language: string): string {
  return languages[language.toLowerCase()] ?? language;
}

function badge(language: string): string {
  const key = language.toLowerCase().replace(/[^a-z0-9]/g, '');
  return `<span class="badge lang-${escapeHtml(key)}">${escapeHtml(languageName(language))}</span>`;
}

function pretty(value: unknown): string {
  if (typeof value === 'string') return value;
  return JSON.stringify(value, null, 2);
}

function pluginRuntime(name: string): string {
  return plugins.find(plugin => plugin.name === name)?.runtime ?? 'wasi-component';
}

function pluginType(name: string): string {
  return plugins.find(plugin => plugin.name === name)?.type ?? 'library';
}

const taskWhy: Record<string, string> = {
  score: 'Rust computes a 0–100 readiness score from the change list and passed checks. Breaking/security entries reduce the score.',
  verdict: 'Rust maps the numeric score to a human decision: ship it, review required, hold, or blocked.',
  pipeline: 'Go calls the Rust readiness interface inside one composed component. This proves build-time cross-language interop.',
  audit: 'Go scans the dependency specs and flags pre-release or unstable versions.',
  changelog: 'TypeScript groups raw changes into breaking/features/fixes/maintenance sections.',
  policy: 'C# applies the policy gate to the same score produced by Rust.',
  'next-version': 'C# calculates the next semantic version from your bump choice.',
  fingerprint: 'C returns a stable FNV-1a fingerprint for the version + change set.',
  metrics: 'C++ measures the generated changelog: characters, words, lines, reading time.',
  save: 'Rust writes the combined report to the host KV database and the reports/ preopen.',
  load: 'Rust reads the saved report back from host KV to prove persistence.',
};

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${HOST}${path}`, {
    ...init,
    headers: { 'content-type': 'application/json', ...(init?.headers ?? {}) },
  });
  const text = await response.text();
  let body: unknown = text;
  try { body = text ? JSON.parse(text) : null; } catch { /* keep text */ }
  if (!response.ok) {
    const message = typeof body === 'string' ? body : JSON.stringify(body);
    throw new Error(`${response.status} ${message}`);
  }
  return body as T;
}

function unwrap(value: unknown): unknown {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    const object = value as Record<string, unknown>;
    if ('ok' in object) return object.ok;
    if ('err' in object) throw new Error(String(object.err));
  }
  return value;
}

async function call(plugin: string, fn: string, args: unknown[]): Promise<unknown> {
  const body = await api<{ result?: unknown; results?: unknown[] }>(
    `/plugin/${encodeURIComponent(plugin)}/call`,
    { method: 'POST', body: JSON.stringify({ function: fn, args }) },
  );
  return unwrap(body.result ?? body.results);
}

async function loadPlugins(): Promise<WasiPlugin[]> {
  plugins = await api<WasiPlugin[]>('/plugins');
  return plugins;
}

async function loadUiPlugins(): Promise<UiPlugin[]> {
  // The digests must come from the same build as the assets. ui-plugins.json
  // is written next to them by `owasidea build`, so a stale desktop binary
  // still verifies the bundle it actually loaded.
  try {
    const response = await fetch('/ui-plugins.json', { cache: 'no-store' });
    if (response.ok) {
      const indexed = await response.json() as UiPlugin[];
      if (Array.isArray(indexed)) {
        uiPlugins = indexed;
        return uiPlugins;
      }
    }
  } catch {
    // Older builds have no bundled index; fall back to the host endpoint.
  }
  uiPlugins = await api<UiPlugin[]>('/ui-plugins');
  return uiPlugins;
}

async function refreshHost(): Promise<void> {
  try {
    const [health, overview] = await Promise.all([
      fetch(`${HOST}/health`).then(response => response.ok ? response.text() : Promise.reject(new Error('offline'))),
      api<Overview & { build_id?: string | null }>('/overview'),
    ]);
    hostPill.className = 'pill online';
    hostPill.innerHTML = `<span class="pulse"></span><span>host ${escapeHtml(health)} · ${overview.plugins} plugins</span>`;
    // RF-08: skew detector.
    const runtimeBid = OWASIDEA_BUILD_ID;
    const hostBid = overview.build_id ?? null;
    let banner = document.getElementById('build-skew-banner');
    if (runtimeBid && hostBid && runtimeBid !== hostBid) {
      if (!banner) {
        banner = document.createElement('div');
        banner.id = 'build-skew-banner';
        banner.className = 'build-skew-banner';
        banner.setAttribute('role', 'alert');
        document.body.insertBefore(banner, document.body.firstChild);
      }
      banner.textContent = `UI bundle anterior ao host (UI: ${runtimeBid}, host: ${hostBid})`;
    } else if (banner) {
      banner.remove();
    }
    const recovered = !hostOnline;
    hostOnline = true;
    // The host starts after the webview in the Tauri shell. If it was not
    // reachable on the first paint, replace the error page once it is up.
    if (recovered && hostErrorShown) {
      hostErrorShown = false;
      route();
    }
  } catch {
    hostOnline = false;
    hostPill.className = 'pill offline';
    hostPill.innerHTML = '<span class="pulse"></span><span>host offline</span>';
  } finally {
    window.clearTimeout(hostRetry);
    hostRetry = window.setTimeout(() => { void refreshHost(); }, hostOnline ? 5000 : 400);
  }
}

function setActiveNav(): void {
  const hash = location.hash || '#/workbench';
  document.querySelectorAll('#nav a').forEach(link => {
    const href = link.getAttribute('href');
    link.classList.toggle('active', href === hash);
  });
}

function closeLogSource(): void {
  logSource?.close();
  logSource = null;
}

function route(): void {
  closeLogSource();
  setActiveNav();
  const hash = location.hash || '#/workbench';
  if (hash === '#/inventory') { void renderInventory(); return; }
  if (hash === '#/runtime') { void renderRuntime(); return; }
  if (hash === '#/ui') { void renderIsolatedUi(); return; }
  if (hash === '#/logs') { void renderLogs(); return; }
  if (hash === '#/dev') { void renderDev(); return; }
  void renderWorkbench();
}

function pageHeader(eyebrow: string, title: string, lede: string): string {
  return `<header class="page-head">
    <div><p class="eyebrow">${escapeHtml(eyebrow)}</p><h1>${escapeHtml(title)}</h1><p class="lede">${escapeHtml(lede)}</p></div>
    <div class="page-actions"><button class="ghost" id="refresh-page">refresh</button></div>
  </header>`;
}

function renderError(error: unknown): void {
  hostErrorShown = true;
  view.innerHTML = `<div class="panel error"><h2>Could not reach the host</h2>
    <p>${escapeHtml(error)}</p>
    <p class="muted">Start the app with <code>owasidea run</code> or <code>owasidea dev</code>, then retry.</p>
    <button id="retry" class="primary">retry</button></div>`;
  view.querySelector('#retry')?.addEventListener('click', () => route());
}

async function renderWorkbench(): Promise<void> {
  view.innerHTML = `${pageHeader('release cockpit', 'Release workbench', 'Fill the release input, then press “run full release”. The dashboard calls six language runtimes through the host HTTP API and shows every WIT call.')}
  <section class="notice">
    <strong>What each element does</strong>
    <p><span class="path-step">Browser</span> → <span class="path-step">POST /plugin/{name}/call</span> → <span class="path-step">owasidea-host (Rust)</span> → <span class="path-step">WASI component</span> → JSON. The browser never links Wasmtime and never executes plugin code directly.</p>
  </section>
  <section class="howto" aria-label="How the release run works">
    <article><b>01</b><div><h3>You provide release input</h3><p>Version, change lines, check counts, and dependency specs. These are sent as JSON arguments.</p></div></article>
    <article><b>02</b><div><h3>Rust scores first</h3><p>core-rust receives the change list and check counts. Its score feeds the C# policy and the Go→Rust composed pipeline.</p></div></article>
    <article><b>03</b><div><h3>Languages run in parallel</h3><p>Go, TypeScript, C#, C, and C++ respond independently. One failure is shown on its own card and does not hide the others.</p></div></article>
    <article><b>04</b><div><h3>Rust persists the report</h3><p>The final Markdown goes to host KV and to the reports/ WASI preopen, then is read back to prove persistence.</p></div></article>
  </section>
  <div class="workbench-grid">
    <section class="panel form-panel">
      <div class="panel-title"><h2>Release input</h2><span class="chip">JSON sent to /call</span></div>
      <div class="field-row">
        <label title="Used by changelog-ts, policy-csharp, pipeline-go, fingerprint-c and report keys">Version <small>release tag sent to every plugin</small><input id="version" value="v0.4.0" spellcheck="false" /></label>
        <label title="Used by policy-csharp next-version">Bump <small>semver choice</small><select id="bump"><option value="patch">patch</option><option value="minor">minor</option><option value="major">major</option></select></label>
        <label title="Passed checks used by the Rust score">Checks passed <small>out of total</small><input id="passed" type="number" min="0" value="9" /></label>
        <label title="Total checks used by the Rust score">Checks total <small>never zero</small><input id="total" type="number" min="1" value="10" /></label>
      </div>
      <label class="field" title="One change per line. Lines containing breaking or security reduce the Rust score.">Changes <small>one per line · “breaking” and “security” lower the score</small><textarea id="changes" rows="6">feat: add isolated React architecture map
fix: retry host health before first render
breaking: change report key format
docs: explain Tauri resource packaging</textarea></label>
      <label class="field" title="One dependency spec per line. The Go audit flags alpha, beta, rc, and 0.0.x.">Dependencies <small>one spec per line · Go flags pre-releases</small><textarea id="deps" rows="3">rust-core@1.4.0
go-audit@0.9.0-beta.2
typescript-jco@1.39.0
policy-csharp@2.0.0</textarea></label>
      <div class="form-actions">
        <button class="primary" id="run-workbench">run full release</button>
        <span id="run-status" class="muted">idle — no calls yet</span>
      </div>
    </section>
    <aside class="panel flow-panel">
      <div class="panel-title"><h2>Execution path</h2><span class="chip live">real calls</span></div>
      <ol class="flow">
        <li><b>01</b><span><strong>core-rust</strong><small>Rust · score + verdict · host KV + reports/</small></span></li>
        <li><b>02</b><span><strong>pipeline-go</strong><small>Go client + Rust provider composed into one WASM component</small></span></li>
        <li><b>03</b><span><strong>audit-go · changelog-ts</strong><small>Go host-log + TypeScript/jco Markdown</small></span></li>
        <li><b>04</b><span><strong>policy-csharp · metrics-cpp · fingerprint-c</strong><small>C#, C++ and C components called in parallel</small></span></li>
        <li><b>05</b><span><strong>core-rust save-report / load-report</strong><small>host KV + filesystem preopen persistence</small></span></li>
      </ol>
      <div class="trace-box">
        <div class="panel-title"><h2>Call trace</h2><span class="chip">live</span></div>
        <pre id="call-trace" class="trace">no calls yet</pre>
      </div>
    </aside>
  </div>
  <section class="panel results-panel">
    <div class="panel-title"><div><h2>Run results</h2><p class="panel-sub">Each card is one HTTP call. It shows the language, plugin, exact WIT function, runtime and returned value.</p></div><span id="result-count" class="chip">not run</span></div>
    <div id="run-results" class="results"><div class="empty-state"><strong>Nothing has run yet.</strong><p>Press “run full release” above. The first call scores the release in Rust; the other cards fill in as components respond.</p></div></div>
  </section>`;
  view.querySelector('#refresh-page')?.addEventListener('click', () => renderWorkbench());
  view.querySelector('#run-workbench')?.addEventListener('click', () => void runWorkbench());
}

async function runWorkbench(): Promise<void> {
  const version = (view.querySelector('#version') as HTMLInputElement).value.trim() || 'v0.0.0';
  const bump = (view.querySelector('#bump') as HTMLSelectElement).value;
  const passed = Number((view.querySelector('#passed') as HTMLInputElement).value || 0);
  const total = Math.max(1, Number((view.querySelector('#total') as HTMLInputElement).value || 1));
  const changes = (view.querySelector('#changes') as HTMLTextAreaElement).value
    .split('\n').map(line => line.trim()).filter(Boolean);
  const deps = (view.querySelector('#deps') as HTMLTextAreaElement).value
    .split('\n').map(line => line.trim()).filter(Boolean);
  const status = view.querySelector('#run-status') as HTMLElement;
  const button = view.querySelector('#run-workbench') as HTMLButtonElement;
  const results = view.querySelector('#run-results') as HTMLElement;
  const traceBox = view.querySelector('#call-trace') as HTMLElement;
  const trace: string[] = [];
  const writeTrace = () => { traceBox.textContent = trace.join('\n') || 'no calls yet'; };
  const addTrace = (line: string) => { trace.push(line); writeTrace(); };
  button.disabled = true;
  status.textContent = 'running — watch the call trace';
  results.innerHTML = '<p class="muted">Waiting for component responses…</p>';
  traceBox.textContent = '';

  const collected: RunResult[] = [];
  const runTask = async (
    key: string, label: string, plugin: string, language: string, fn: string, args: unknown[],
  ): Promise<RunResult> => {
    const started = performance.now();
    addTrace(`→ POST /plugin/${plugin}/call { function: "${fn}" }`);
    try {
      const value = await call(plugin, fn, args);
      const ms = Math.round(performance.now() - started);
      addTrace(`← 200 ${plugin}.${fn} (${ms} ms)`);
      return {
        key, label, plugin, language, function: fn, value, ms,
        why: taskWhy[key] ?? 'WIT function called through the host.',
        runtime: pluginRuntime(plugin),
      };
    } catch (error) {
      const ms = Math.round(performance.now() - started);
      addTrace(`← ERROR ${plugin}.${fn} (${ms} ms)`);
      return {
        key, label, plugin, language, function: fn, error: String(error), ms,
        why: taskWhy[key] ?? 'WIT function called through the host.',
        runtime: pluginRuntime(plugin),
      };
    }
  };

  const scoreResult = await runTask('score', 'Readiness score', 'core-rust', 'rust', 'score', [changes, passed, total]);
  collected.push(scoreResult);
  const score = typeof scoreResult.value === 'number' ? scoreResult.value : 0;

  const parallel = await Promise.all([
    runTask('verdict', 'Verdict', 'core-rust', 'rust', 'verdict', [score]),
    runTask('pipeline', 'Composed pipeline', 'pipeline-go', 'go', 'run', [version, changes, passed, total]),
    runTask('audit', 'Dependency audit', 'audit-go', 'go', 'scan', [deps]),
    runTask('changelog', 'Changelog renderer', 'changelog-ts', 'typescript', 'render', [version, changes]),
    runTask('policy', 'Policy classification', 'policy-csharp', 'csharp', 'classify', [score]),
    runTask('next-version', 'Next semantic version', 'policy-csharp', 'csharp', 'next-version', [version, bump]),
    runTask('fingerprint', 'Release fingerprint', 'fingerprint-c', 'c', 'fingerprint', [`${version}\n${changes.join('\n')}`]),
  ]);
  collected.push(...parallel);

  const changelog = parallel.find(result => result.key === 'changelog')?.value;
  const changelogText = typeof changelog === 'string' ? changelog : JSON.stringify(changelog ?? '');
  collected.push(await runTask('metrics', 'Changelog metrics', 'metrics-cpp', 'cpp', 'analyze', [changelogText]));

  const verdict = parallel.find(result => result.key === 'verdict')?.value;
  const policy = parallel.find(result => result.key === 'policy')?.value;
  const report = [
    `# Shipyard release report — ${version}`,
    '',
    `- readiness score: **${score}**`,
    `- verdict: **${String(verdict ?? 'unknown')}**`,
    `- policy: **${String(policy ?? 'unknown')}**`,
    `- changes: ${changes.length}`,
    `- checks: ${passed}/${total}`,
    '',
    '## Changelog',
    '',
    changelogText,
  ].join('\n');

  collected.push(await runTask('save', 'Persist report', 'core-rust', 'rust', 'save-report', [version, report]));
  collected.push(await runTask('load', 'Read persisted report', 'core-rust', 'rust', 'load-report', [version]));

  renderRunResults(collected);
  const failures = collected.filter(result => result.error).length;
  status.textContent = failures ? `completed with ${failures} failed call(s)` : `completed — ${collected.length} calls succeeded`;
  button.disabled = false;
}

function renderRunResults(results: RunResult[]): void {
  const container = view.querySelector('#run-results')!;
  const count = view.querySelector('#result-count')!;
  count.textContent = `${results.length} calls`;
  container.innerHTML = results.map(result => {
    const value = result.error
      ? `<span class="negative">${escapeHtml(result.error)}</span>`
      : `<pre>${escapeHtml(pretty(result.value))}</pre>`;
    return `<article class="result-card ${result.error ? 'has-error' : ''}" data-call="${escapeHtml(result.plugin)}.${escapeHtml(result.function)}">
      <div class="result-head">
        ${badge(result.language)}
        <div><h3>${escapeHtml(result.label)}</h3><small>${escapeHtml(result.plugin)} · <code>${escapeHtml(result.function)}</code></small></div>
        <span class="status ${result.error ? 'negative' : 'positive'}">${result.error ? 'error' : 'ok'}</span>
      </div>
      <p class="result-why">${escapeHtml(result.why)}</p>
      <div class="result-meta"><code>POST /plugin/${escapeHtml(result.plugin)}/call</code><span>${result.ms} ms</span><span>${escapeHtml(result.runtime)}</span></div>
      ${value}
    </article>`;
  }).join('');
}

async function renderInventory(): Promise<void> {
  try {
    const [overview] = await Promise.all([api<Overview>('/overview'), loadPlugins()]);
    view.innerHTML = `${pageHeader('runtime inventory', 'Plugins and contracts', 'Every card is a generated manifest. WASI components are loaded by the Rust host; UI plugins are webview-only bundles. Generated manifests are the source of truth for exports, imports and permissions.')}
    <section class="notice">
      <strong>How to read this screen</strong>
      <p><b>library</b> = a fresh component instance per <code>/call</code>. <b>service</b> = one persistent instance with start/status/stop. <b>executable</b> = mounted at <code>/app/{name}</code> through <code>wasi:http</code>. <b>ui</b> = static bundle in a sandboxed iframe, never loaded by Wasmtime.</p>
    </section>
    <div class="stat-row">
      <div class="stat"><strong>${overview.plugins}</strong><span>headless plugins</span><small>loaded by Wasmtime</small></div>
      <div class="stat"><strong>${overview.libraries}</strong><span>libraries</span><small>per-call instances</small></div>
      <div class="stat"><strong>${overview.executables}</strong><span>executables</span><small>wasi:http inbound</small></div>
      <div class="stat"><strong>${overview.services}</strong><span>services</span><small>persistent lifecycle</small></div>
      <div class="stat"><strong>${overview.runtimes.map(escapeHtml).join(' + ')}</strong><span>runtimes</span><small>WASI Component Model</small></div>
    </div>
    <div class="inventory-grid">
      ${plugins.map(plugin => {
        const meta = pluginMeta[plugin.name] ?? { role: 'WIT component', why: 'Generated manifest component.', detail: plugin.runtime };
        return `<article class="panel plugin-card" data-plugin="${escapeHtml(plugin.name)}">
        <div class="plugin-top">${badge(plugin.language)}<span class="chip" title="Plugin type: library, service, executable or ui">${escapeHtml(plugin.type)}</span></div>
        <h2>${escapeHtml(plugin.name)}</h2>
        <p class="plugin-role">${escapeHtml(meta.role)}</p>
        <p class="plugin-why">${escapeHtml(meta.why)}</p>
        <div class="meta-line"><code>${escapeHtml(plugin.runtime)}</code><span>v${escapeHtml(plugin.version)}</span></div>
        <div class="export-block"><small>WIT exports</small><ul class="export-list">${plugin.exports.slice(0, 5).map(exp => `<li><code>${escapeHtml(exp)}</code><span>function / interface</span></li>`).join('')}</ul></div>
        <button class="ghost inspect" data-plugin="${escapeHtml(plugin.name)}" title="Open the generated manifest for ${escapeHtml(plugin.name)}">inspect generated manifest</button>
      </article>`;
      }).join('')}
    </div>
    <section id="manifest-panel" class="panel manifest-panel hidden"><div class="panel-title"><div><h2>Generated manifest</h2><p class="panel-sub">Imports, exports, permissions, checksum and signature. Never edit this file by hand; rebuild it from WIT.</p></div><button class="ghost" id="close-manifest">close</button></div><pre id="manifest-json"></pre></section>`;
    view.querySelector('#refresh-page')?.addEventListener('click', () => renderInventory());
    view.querySelector('#close-manifest')?.addEventListener('click', () => {
      view.querySelector('#manifest-panel')?.classList.add('hidden');
    });
    view.querySelectorAll<HTMLButtonElement>('button.inspect').forEach(button => {
      button.addEventListener('click', async () => {
        const name = button.dataset.plugin!;
        const manifest = await api<unknown>(`/plugin/${encodeURIComponent(name)}/manifest`);
        (view.querySelector('#manifest-json') as HTMLElement).textContent = JSON.stringify(manifest, null, 2);
        view.querySelector('#manifest-panel')?.classList.remove('hidden');
        view.querySelector('#manifest-panel')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
      });
    });
  } catch (error) {
    renderError(error);
  }
}

async function renderRuntime(): Promise<void> {
  try {
    const overview = await api<Overview>('/overview');
    view.innerHTML = `${pageHeader('operations', 'Runtime controls', 'Two very different plugin models on one host: a stateful service that keeps an instance between calls, and an executable plugin that handles real HTTP requests.')}
    <section class="notice">
      <strong>What the buttons do</strong>
      <p><b>watchtower-rust</b>: <code>start</code> creates a persistent instance, <code>status</code> queries the same instance, <code>stop</code> drops it. <b>webhook-go</b>: every click is a real HTTP request routed to <code>wasi:http/incoming-handler</code>.</p>
    </section>
    <div class="runtime-grid">
      <section class="panel">
        <div class="panel-title"><div><h2>watchtower-rust</h2><p class="panel-sub">Persistent service lifecycle</p></div><span class="chip">type: service</span></div>
        <p class="help">The host keeps one component instance alive after <code>start</code>. This is why hot reload returns 409 while it runs: stop it before replacing the component.</p>
        <div class="button-row">
          <button class="primary" id="service-start" title="POST /plugin/watchtower-rust/service/start">start persistent instance</button>
          <button class="ghost" id="service-status" title="GET /plugin/watchtower-rust/service/status">query status</button>
          <button class="danger" id="service-stop" title="POST /plugin/watchtower-rust/service/stop">stop and drop instance</button>
        </div>
        <pre id="service-output" class="output">not started — press “start persistent instance”</pre>
      </section>
      <section class="panel">
        <div class="panel-title"><div><h2>webhook-go</h2><p class="panel-sub">Executable wasi:http plugin</p></div><span class="chip">type: executable</span></div>
        <p class="help">Mounted at <code>/app/webhook-go</code>. The host forwards method, headers, body and response; no <code>wasmtime serve</code> child process is spawned.</p>
        <div class="button-row">
          <button class="primary" id="webhook-health" title="GET /app/webhook-go/health">GET /health</button>
          <button class="ghost" id="webhook-event" title="POST /app/webhook-go/events">POST /events</button>
        </div>
        <pre id="webhook-output" class="output">no request yet</pre>
      </section>
      <section class="panel">
        <div class="panel-title"><div><h2>Host overview</h2><p class="panel-sub">Generated at startup</p></div><span class="chip">${escapeHtml(overview.runtimes.join(' + '))}</span></div>
        <p class="help">Counts come from the manifests the host loaded. Every headless plugin was instantiated once as a startup smoke test.</p>
        <pre class="output">${escapeHtml(JSON.stringify(overview, null, 2))}</pre>
      </section>
      <section class="panel">
        <div class="panel-title"><div><h2>Dev status</h2><p class="panel-sub">Watcher and last reload</p></div><span class="chip" id="dev-chip">loading</span></div>
        <p class="help">Only available in <code>owasidea dev</code>. Shows watch roots and the result of explicit reloads; production run has no watcher.</p>
        <pre id="dev-output" class="output">loading…</pre>
      </section>
    </div>`;
    view.querySelector('#refresh-page')?.addEventListener('click', () => renderRuntime());
    const serviceOutput = view.querySelector('#service-output') as HTMLElement;
    const service = async (action: 'start' | 'status' | 'stop') => {
      try {
        const method = action === 'status' ? 'GET' : 'POST';
        const response = await fetch(`${HOST}/plugin/watchtower-rust/service/${action}`, {
          method,
          headers: { 'content-type': 'application/json' },
          ...(action === 'start' ? { body: JSON.stringify({ region_json: '' }) } : {}),
        });
        serviceOutput.textContent = `service ${action}\n${method} /plugin/watchtower-rust/service/${action}\n\n${JSON.stringify(await response.json(), null, 2)}`;
      } catch (error) {
        serviceOutput.textContent = String(error);
      }
    };
    view.querySelector('#service-start')?.addEventListener('click', () => void service('start'));
    view.querySelector('#service-status')?.addEventListener('click', () => void service('status'));
    view.querySelector('#service-stop')?.addEventListener('click', () => void service('stop'));

    const webhookOutput = view.querySelector('#webhook-output') as HTMLElement;
    view.querySelector('#webhook-health')?.addEventListener('click', async () => {
      const response = await fetch(`${HOST}/app/webhook-go/health`);
      webhookOutput.textContent = `GET /app/webhook-go/health\nHTTP ${response.status}\n\n${await response.text()}`;
    });
    view.querySelector('#webhook-event')?.addEventListener('click', async () => {
      const response = await fetch(`${HOST}/app/webhook-go/events`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ type: 'release.observed', component: 'shipyard-ui', payload: { version: 'v0.4.0' } }),
      });
      webhookOutput.textContent = `POST /app/webhook-go/events\nHTTP ${response.status}\n\n${await response.text()}`;
    });

    const dev = await api<DevStatus>('/dev/status').catch(() => null);
    const devChip = view.querySelector('#dev-chip')!;
    const devOutput = view.querySelector('#dev-output')!;
    if (dev) {
      devChip.textContent = dev.active ? 'active' : 'production';
      devOutput.textContent = JSON.stringify(dev, null, 2);
    } else {
      devChip.textContent = 'unavailable';
      devOutput.textContent = 'GET /dev/status failed';
    }
  } catch (error) {
    renderError(error);
  }
}

function sha256Fallback(bytes: Uint8Array): string {
  const K = new Uint32Array([
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ]);
  const rotr = (value: number, bits: number) => (value >>> bits) | (value << (32 - bits));
  let h0 = 0x6a09e667, h1 = 0xbb67ae85, h2 = 0x3c6ef372, h3 = 0xa54ff53a;
  let h4 = 0x510e527f, h5 = 0x9b05688c, h6 = 0x1f83d9ab, h7 = 0x5be0cd19;
  const total = ((bytes.length + 9 + 63) >> 6) << 6;
  const padded = new Uint8Array(total);
  padded.set(bytes);
  padded[bytes.length] = 0x80;
  const view = new DataView(padded.buffer);
  const bits = bytes.length * 8;
  view.setUint32(total - 8, Math.floor(bits / 0x100000000), false);
  view.setUint32(total - 4, bits >>> 0, false);
  const w = new Uint32Array(64);
  for (let offset = 0; offset < total; offset += 64) {
    for (let i = 0; i < 16; i++) w[i] = view.getUint32(offset + i * 4, false);
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }
    let a = h0, b = h1, c = h2, d = h3, e = h4, f = h5, g = h6, h = h7;
    for (let i = 0; i < 64; i++) {
      const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (h + s1 + ch + K[i] + w[i]) >>> 0;
      const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (s0 + maj) >>> 0;
      h = g; g = f; f = e; e = (d + t1) >>> 0;
      d = c; c = b; b = a; a = (t1 + t2) >>> 0;
    }
    h0 = (h0 + a) >>> 0; h1 = (h1 + b) >>> 0; h2 = (h2 + c) >>> 0; h3 = (h3 + d) >>> 0;
    h4 = (h4 + e) >>> 0; h5 = (h5 + f) >>> 0; h6 = (h6 + g) >>> 0; h7 = (h7 + h) >>> 0;
  }
  return [h0, h1, h2, h3, h4, h5, h6, h7]
    .map(value => value.toString(16).padStart(8, '0')).join('');
}

async function sha256Hex(data: BufferSource): Promise<string> {
  if (globalThis.crypto?.subtle) {
    const digest = await crypto.subtle.digest('SHA-256', data);
    return Array.from(new Uint8Array(digest)).map(byte => byte.toString(16).padStart(2, '0')).join('');
  }
  let bytes: Uint8Array;
  if (data instanceof ArrayBuffer) {
    bytes = new Uint8Array(data);
  } else {
    bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }
  return sha256Fallback(bytes);
}

async function verifyUiPlugin(plugin: UiPlugin): Promise<'WebCrypto' | 'JS fallback'> {
  const mode = globalThis.crypto?.subtle ? 'WebCrypto' : 'JS fallback';
  const base = `/plugins/${encodeURIComponent(plugin.name)}/`;
  const lines: string[] = [];
  for (const path of Object.keys(plugin.files).sort()) {
    const response = await fetch(base + path.split('/').map(encodeURIComponent).join('/'), { cache: 'no-store' });
    if (!response.ok) throw new Error(`missing asset ${path}`);
    const bytes = await response.arrayBuffer();
    const digest = await sha256Hex(bytes);
    if (digest !== plugin.files[path]) throw new Error(`digest mismatch ${path}: expected ${plugin.files[path]}, got ${digest}`);
    lines.push(`${path}\0${digest}\n`);
  }
  const digest = await sha256Hex(new TextEncoder().encode(lines.join('')));
  if (digest !== plugin.sha256) throw new Error(`bundle digest mismatch: expected ${plugin.sha256}, got ${digest}`);
  return mode;
}

async function renderIsolatedUi(): Promise<void> {
  try {
    await loadUiPlugins();
    if (uiPlugins.length === 0) {
      view.innerHTML = `${pageHeader('micro-frontends', 'Isolated UI plugins', 'UI plugins are separate static bundles. None are installed in this build.')}
      <section class="panel empty-state"><strong>No UI plugins found.</strong><p>Run <code>owasidea build</code> so the Svelte and React bundles are copied into <code>ui/public/plugins/</code> and their manifests are generated.</p></section>`;
      return;
    }
    view.innerHTML = `${pageHeader('micro-frontends', 'Isolated UI plugins', 'Each panel is a separate Svelte or React build. The dashboard verifies every asset hash before mounting it in a sandboxed iframe.')}
    <section class="notice">
      <strong>Verification and isolation flow</strong>
      <p><b>1.</b> Read <code>ui-plugins.json</code> from the same bundle as the assets · <b>2.</b> fetch every listed asset · <b>3.</b> verify per-file SHA-256 and the aggregate bundle hash · <b>4.</b> mount only after success. The iframe has no same-origin access and no WASI capabilities; it receives a host bridge only when the plugin declares an allowlist.</p>
    </section>
    <div class="ui-grid">
      ${uiPlugins.map(plugin => {
        const meta = pluginMeta[plugin.name] ?? { role: 'Webview UI bundle', why: 'Static UI bundle.', detail: 'webview only' };
        return `<article class="panel ui-card" data-ui="${escapeHtml(plugin.name)}">
        <div class="plugin-top"><span class="badge lang-${escapeHtml(plugin.language)}">${escapeHtml(languageName(plugin.language))}</span><span class="chip">js-wasm · iframe</span></div>
        <h2>${escapeHtml(plugin.name)}</h2>
        <p class="plugin-role">${escapeHtml(meta.role)}</p>
        <p class="plugin-why">${escapeHtml(meta.why)}</p>
        <div class="meta-line"><span>${Object.keys(plugin.files).length} assets</span><code>${escapeHtml(plugin.entry)}</code></div>
        <p class="verify" data-verify="${escapeHtml(plugin.name)}">verifying assets…</p>
        <button class="primary open-ui" data-ui="${escapeHtml(plugin.name)}" disabled title="Available after every asset hash matches the manifest">open verified panel</button>
      </article>`;
      }).join('')}
    </div>
    <section id="ui-frame-wrap" class="panel hidden">
      <div class="panel-title"><div><h2 id="ui-frame-title">Isolated panel</h2><p class="panel-sub">This iframe is a separate bundle. It cannot read the dashboard DOM or call the host API directly; only its allowlisted bridge exports are reachable.</p></div><button class="ghost" id="close-ui">close</button></div>
      <div id="ui-frame"></div>
    </section>`;
    view.querySelector('#refresh-page')?.addEventListener('click', () => renderIsolatedUi());
    for (const plugin of uiPlugins) {
      const label = view.querySelector(`[data-verify="${CSS.escape(plugin.name)}"]`) as HTMLElement;
      const button = view.querySelector(`button.open-ui[data-ui="${CSS.escape(plugin.name)}"]`) as HTMLButtonElement;
      verifyUiPlugin(plugin)
        .then(mode => {
          label.textContent = `✓ ${Object.keys(plugin.files).length} assets verified · ${plugin.sha256.slice(0, 12)}… · ${mode}`;
          label.className = 'verify positive';
          button.disabled = false;
        })
        .catch(error => {
          label.textContent = `✗ verification failed: ${String(error)}`;
          label.className = 'verify negative';
          button.disabled = true;
        });
    }
    view.querySelectorAll<HTMLButtonElement>('button.open-ui').forEach(button => {
      button.addEventListener('click', async () => {
        const name = button.dataset.ui!;
        const plugin = uiPlugins.find(item => item.name === name)!;
        try {
          const mode = await verifyUiPlugin(plugin);
          const frame = document.createElement('iframe');
          frame.title = `${plugin.name} isolated UI`;
          frame.src = `/${plugin.entry.split('/').map(encodeURIComponent).join('/')}`;
          frame.setAttribute('sandbox', 'allow-scripts allow-forms');
          frame.referrerPolicy = 'no-referrer';
          frame.style.cssText = 'width:100%;height:720px;border:1px solid #2a3954;border-radius:12px;background:#0b1020';
          (view.querySelector('#ui-frame-title') as HTMLElement).textContent = `${plugin.name} — ${plugin.language} — verified with ${mode}`;
          const host = view.querySelector('#ui-frame')!;
          host.replaceChildren(frame);
          if (Array.isArray(plugin.bridge) && plugin.bridge.length) {
            attachBridge(frame, plugin as any, HOST);
          }
          view.querySelector('#ui-frame-wrap')!.classList.remove('hidden');
          view.querySelector('#ui-frame-wrap')!.scrollIntoView({ behavior: 'smooth', block: 'start' });
        } catch (error) {
          const wrap = view.querySelector('#ui-frame-wrap')!;
          wrap.classList.remove('hidden');
          (view.querySelector('#ui-frame-title') as HTMLElement).textContent = `${name} — integrity error`;
          (view.querySelector('#ui-frame') as HTMLElement).innerHTML = `<pre class="negative">${escapeHtml(error)}</pre>`;
        }
      });
    });
    view.querySelector('#close-ui')?.addEventListener('click', () => {
      (view.querySelector('#ui-frame') as HTMLElement).replaceChildren();
      view.querySelector('#ui-frame-wrap')?.classList.add('hidden');
    });
  } catch (error) {
    renderError(error);
  }
}

async function renderLogs(): Promise<void> {
  try {
    await loadPlugins();
    const headless = plugins.filter(plugin => plugin.runtime !== 'js-wasm');
    view.innerHTML = `${pageHeader('observability', 'Component logs', 'Logs are emitted by components through the host-log WIT import. The host keeps a bounded ring buffer and streams new lines over SSE.')}
    <section class="notice">
      <strong>Where these lines come from</strong>
      <p>Components call <code>log()</code> in <code>poc:host/host-log</code>. The host stores up to 1,000 lines per plugin, truncates long messages, and streams only new entries. This is not stdout from a child process.</p>
    </section>
    <section class="panel logs-panel">
      <div class="panel-title"><div><h2>Live stream</h2><p class="panel-sub">Select a plugin; history is loaded first, then new lines arrive over SSE.</p></div><select id="log-plugin" title="Choose which plugin's ring buffer to inspect">${headless.map(plugin => `<option value="${escapeHtml(plugin.name)}">${escapeHtml(plugin.name)} — ${escapeHtml(plugin.type)}</option>`).join('')}</select></div>
      <pre id="log-output" class="logs">connecting…</pre>
      <div class="button-row"><button class="ghost" id="clear-logs" title="Clear only this browser view">clear view</button><button class="ghost" id="reload-logs" title="Reload history and reconnect the SSE stream">reconnect stream</button></div>
    </section>`;
    view.querySelector('#refresh-page')?.addEventListener('click', () => renderLogs());
    const select = view.querySelector('#log-plugin') as HTMLSelectElement;
    const output = view.querySelector('#log-output') as HTMLElement;
    const connect = async () => {
      closeLogSource();
      output.textContent = 'loading history…';
      const history = await api<{ logs: string[] }>(`/plugin/${encodeURIComponent(select.value)}/logs`);
      output.textContent = history.logs.length ? history.logs.join('\n') : '(no log lines yet)';
      logSource = new EventSource(`${HOST}/plugin/${encodeURIComponent(select.value)}/logs/stream`);
      logSource.onmessage = event => {
        output.textContent = `${output.textContent}\n${event.data}`;
        output.scrollTop = output.scrollHeight;
      };
      logSource.onerror = () => { output.textContent += '\n[stream disconnected]'; };
    };
    select.addEventListener('change', () => void connect());
    view.querySelector('#clear-logs')?.addEventListener('click', () => { output.textContent = ''; });
    view.querySelector('#reload-logs')?.addEventListener('click', () => void connect());
    await connect();
  } catch (error) {
    renderError(error);
  }
}

async function renderDev(): Promise<void> {
  try {
    const status = await api<DevStatus>('/dev/status');
    await loadPlugins();
    const reloadable = plugins.filter(plugin => plugin.runtime !== 'js-wasm');
    view.innerHTML = `${pageHeader('developer loop', 'Dev mode', 'owasidea dev builds once, then watches plugins and manifests. Reloading re-reads the manifest and component bytes; stateful services must be stopped first.')}
    <section class="notice">
      <strong>What the watcher does</strong>
      <p>Source edits are reported but still require <code>owasidea build</code>. A changed <code>.component.wasm</code> or manifest can be reloaded immediately. Reloading a running service returns HTTP 409 by design.</p>
    </section>
    <div class="stat-row">
      <div class="stat"><strong>${status.active ? 'on' : 'off'}</strong><span>dev mode</span><small>OWASIDEA_DEV_MODE</small></div>
      <div class="stat"><strong>${status.hot_reload ? 'on' : 'off'}</strong><span>hot reload</span><small>auto-reload on artifact change</small></div>
      <div class="stat"><strong>${status.watch_paths.length}</strong><span>watch roots</span><small>plugins + manifests + UI</small></div>
    </div>
    <section class="panel">
      <div class="panel-title"><div><h2>Watch paths</h2><p class="panel-sub">Recursive; source trees, target, node_modules and .git are skipped.</p></div><span class="chip">recursive</span></div>
      <ul class="paths">${status.watch_paths.map(path => `<li><code>${escapeHtml(path)}</code></li>`).join('')}</ul>
    </section>
    <section class="panel">
      <div class="panel-title"><div><h2>Reload components</h2><p class="panel-sub">POST /plugin/{name}/reload re-reads the manifest and component, then re-instantiates.</p></div><span class="chip">explicit action</span></div>
      <div class="reload-grid">
        ${reloadable.map(plugin => {
          const last = status.last_reload[plugin.name];
          const state = last ? (last.ok ? `ok · ${last.http_status}` : `failed · HTTP ${last.http_status}`) : 'not reloaded';
          return `<div class="reload-card"><div>${badge(plugin.language)}<strong>${escapeHtml(plugin.name)}</strong><small>${escapeHtml(state)}</small></div><button class="ghost" data-reload="${escapeHtml(plugin.name)}">reload</button></div>`;
        }).join('')}
      </div>
    </section>
    <pre id="dev-result" class="output">idle</pre>`;
    view.querySelector('#refresh-page')?.addEventListener('click', () => renderDev());
    view.querySelectorAll<HTMLButtonElement>('button[data-reload]').forEach(button => {
      button.addEventListener('click', async () => {
        const name = button.dataset.reload!;
        const output = view.querySelector('#dev-result') as HTMLElement;
        output.textContent = `reloading ${name}…`;
        try {
          const response = await fetch(`${HOST}/plugin/${encodeURIComponent(name)}/reload`, { method: 'POST' });
          output.textContent = `${response.status} ${await response.text()}`;
        } catch (error) {
          output.textContent = String(error);
        }
      });
    });
  } catch (error) {
    renderError(error);
  }
}

window.addEventListener('hashchange', route);
window.addEventListener('DOMContentLoaded', () => {
  hostPill.className = 'pill offline';
  hostPill.innerHTML = '<span class="pulse"></span><span>connecting…</span>';
  void refreshHost();
  route();
});
