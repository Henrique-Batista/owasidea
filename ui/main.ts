import { attachBridge } from './bridge.ts';
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
let uiPluginReloadTimer: number | null = null;

function escapeHtml(value: unknown): string {
  return String(value).replace(/[&<>"']/g, ch => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  })[ch]!);
}

function route() {
  const hash = location.hash || '#/overview';
  if (hash.startsWith('#/ui/')) {
    renderUiPlugin(decodeURIComponent(hash.slice('#/ui/'.length)));
  } else if (hash === '#/ui-plugins') {
    renderUiPlugins();
  } else if (hash.startsWith('#/plugins/')) {
    const name = decodeURIComponent(hash.slice('#/plugins/'.length));
    renderPluginDetail(name);
  } else if (hash === '#/plugins') {
    renderPlugins();
  } else if (hash === '#/services') {
    renderServices();
  } else if (hash === '#/dev') {
    renderDev();
  } else {
    renderOverview();
  }
}

async function renderOverview() {
  let health = 'unreachable';
  try { health = await fetch(`${HOST}/health`).then(r => r.text()); } catch {}
  const ov = await fetch(`${HOST}/overview`).then(r => r.json()).catch(() => ({}));
  const runtimeBuildId = OWASIDEA_BUILD_ID;
  const hostBuildId = (ov.build_id ?? null) as string | null;
  let skewBanner = '';
  if (runtimeBuildId && hostBuildId && runtimeBuildId !== hostBuildId) {
    skewBanner = `<div class="build-skew-banner" role="alert">UI bundle anterior ao host (UI: ${escapeHtml(runtimeBuildId)}, host: ${escapeHtml(hostBuildId)})</div>`;
  }
  const buildId = runtimeBuildId ?? hostBuildId ?? null;
  view.innerHTML = `${skewBanner}<h1>Overview</h1>
    <div class="card">
      <p>host: ${health}</p>
      <p>host_url: ${escapeHtml(HOST)}</p>
      <p>plugins: ${ov.plugins ?? 0} (libraries: ${ov.libraries ?? 0}, executables: ${ov.executables ?? 0}, services: ${ov.services ?? 0})</p>
      <p>runtimes: ${(ov.runtimes || []).map(escapeHtml).join(', ')}</p>
      <p>build_id: ${escapeHtml(buildId ?? 'n/a')}</p>
    </div>`;
}

async function renderPlugins() {
  const plugins = await fetch(`${HOST}/plugins`).then(r => r.json());
  view.innerHTML = `<h1>Plugins</h1>` + plugins.map((p: any) =>
    `<div class="card"><h2><a href="#/plugins/${encodeURIComponent(p.name)}">${escapeHtml(p.name)}</a> <small>(${escapeHtml(p.language)}, ${escapeHtml(p.type)}, ${escapeHtml(p.runtime)})</small></h2>
     <p>exports: ${p.exports.map(escapeHtml).join(', ')}</p></div>`).join('');
}

async function renderPluginDetail(name: string) {
  const encodedName = encodeURIComponent(name);
  const manifest = await fetch(`${HOST}/plugin/${encodedName}/manifest`).then(r => r.json());
  const logs = await fetch(`${HOST}/plugin/${encodedName}/logs`).then(r => r.json()).catch(() => ({ logs: [], entries: [] }));
  const diff = await fetch(`${HOST}/permissions/diff`).then(r => r.json()).catch(() => ({ plugins: {} }));
  const exports = (manifest.exports || []).filter((exp: any) => exp.signature !== 'interface');
  const pluginDiff = (diff.plugins || {})[name] || { added: [], removed: [] };
  let diffHtml = '';
  if ((pluginDiff.added || []).length || (pluginDiff.removed || []).length) {
    diffHtml = `<div class="card"><h2>Permission changes</h2><ul>` +
      (pluginDiff.added || []).map((x: string) => `<li style="color:#7f7">+ ${escapeHtml(x)}</li>`).join('') +
      (pluginDiff.removed || []).map((x: string) => `<li style="color:#f77">- ${escapeHtml(x)}</li>`).join('') +
      `</ul></div>`;
  }
  let html = `<h1>Plugin: ${escapeHtml(name)}</h1>
    <button data-reload="1">reload plugin</button>
    ${diffHtml}
    <div class="card"><h2>Manifest</h2><pre>${escapeHtml(JSON.stringify(manifest, null, 2))}</pre></div>
    <div class="card"><h2>Exports</h2>`;
  for (const exp of (manifest.exports || [])) {
    if (exp.signature === 'interface') continue;
    const params = exp.params || [];
    html += `<div class="export" data-export="${escapeHtml(exp.name)}" style="margin:0.75rem 0;padding:0.5rem;border:1px solid #4443;border-radius:6px">
      <strong>${escapeHtml(exp.name)}</strong> <code>${escapeHtml(exp.signature)}</code><br/>`;
    if (params.length) {
      for (const p of params) {
        const t = String(p.type || '');
        const lower = t.toLowerCase();
        const isBool = lower === 'bool' || lower === 'boolean';
        const isNumber = /^(i|u|f)\d*$/.test(lower) || lower.includes('int') || lower.includes('float') || lower.includes('double') || lower === 'number';
        const common = `data-param="${escapeHtml(p.name)}" data-input="${escapeHtml(p.name)}" name="${escapeHtml(p.name)}"`;
        html += `<label style="display:block;margin:0.35rem 0">${escapeHtml(p.name)} <small>(${escapeHtml(t)})</small><br/>`;
        if (isBool) {
          html += `<input type="checkbox" ${common} />`;
        } else if (isNumber) {
          html += `<input type="number" ${common} step="any" style="width:60%" />`;
        } else if (lower === 'string' || lower === 'str' || lower === 'char') {
          html += `<input type="text" ${common} style="width:60%" />`;
        } else {
          html += `<textarea ${common} rows="3" style="width:60%" placeholder='JSON value for ${escapeHtml(t)}'></textarea>
            <div><small>hint: ${escapeHtml(t)} (JSON)</small></div>`;
        }
        html += `</label>`;
      }
    } else {
      html += `<label style="display:block;margin:0.35rem 0">args <small>(comma separated)</small><br/>
        <input type="text" data-legacy="1" data-input="${escapeHtml(exp.name)}" style="width:60%" /></label>`;
    }
    html += `<button data-call="${escapeHtml(exp.name)}" data-invoke="${escapeHtml(exp.name)}">Invoke</button>
      <span data-latency="${escapeHtml(exp.name)}" style="margin-left:0.5rem"></span>
      <pre data-out="${escapeHtml(exp.name)}" data-result="${escapeHtml(exp.name)}" style="margin-top:0.5rem"></pre>
      <pre data-error="${escapeHtml(exp.name)}" style="color:#c33"></pre>
      <details style="margin-top:0.5rem"><summary>snippets</summary>
        <pre data-snippet-fetch="${escapeHtml(exp.name)}"></pre>
        <pre data-snippet-curl="${escapeHtml(exp.name)}"></pre>
      </details>
    </div>`;
  }
  if (!exports.length) html += `<p>No callable exports.</p>`;
  html += `</div>
    <div class="card"><h2>Logs</h2><pre>${escapeHtml(((logs.entries || logs.logs || []).map((e: any) => typeof e === 'string' ? e : `${e.ts} ${e.level} [${e.request_id}] ${e.message}`).join('\n')))}</pre>
      <button id="showlogs">refresh logs</button></div>`;
  view.innerHTML = html;

  const parseField = (el: Element | null, type: string): { ok: boolean; value?: any; error?: string } => {
    const t = type.toLowerCase();
    if (!el) return { ok: false, error: 'missing field' };
    if (el instanceof HTMLInputElement && el.type === 'checkbox') {
      return { ok: true, value: el.checked };
    }
    if (el instanceof HTMLInputElement && el.type === 'number') {
      const raw = el.value.trim();
      if (raw === '') return { ok: false, error: 'required' };
      const n = Number(raw);
      if (Number.isNaN(n)) return { ok: false, error: 'not a number' };
      return { ok: true, value: n };
    }
    if (el instanceof HTMLTextAreaElement) {
      const raw = el.value.trim();
      if (raw === '') return { ok: false, error: 'required' };
      try { return { ok: true, value: JSON.parse(raw) }; }
      catch { return { ok: false, error: 'invalid JSON' }; }
    }
    // text
    const val = (el as HTMLInputElement).value;
    if (t === 'string' || t === 'str' || t === 'char') {
      return { ok: true, value: val };
    }
    // complex as text? try JSON
    try { return { ok: true, value: JSON.parse(val) }; }
    catch { return { ok: true, value: val }; }
  };

  view.querySelectorAll('button[data-call]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const fn = (btn as HTMLElement).getAttribute('data-call')!;
      const exp = (manifest.exports || []).find((e: any) => e.name === fn);
      const params = exp?.params || [];
      const args: any[] = [];
      let validationError: string | null = null;
      if (params.length) {
        for (const p of params) {
          const sel = `[data-export="${CSS.escape(fn)}"] [data-param="${CSS.escape(p.name)}"]`;
          const el = view.querySelector(sel);
          const parsed = parseField(el, String(p.type || ''));
          if (!parsed.ok) { validationError = `${p.name}: ${parsed.error}`; break; }
          args.push(parsed.value);
        }
      } else {
        const input = view.querySelector(`input[data-input="${CSS.escape(fn)}"][data-legacy="1"]`) as HTMLInputElement | null;
        const raw = input?.value.trim() ?? '';
        const coerced = raw === '' ? [] : raw.split(',').map(s => s.trim()).map(a => {
          if (a === 'true') return true;
          if (a === 'false') return false;
          if (/^-?\d+$/.test(a)) return Number(a);
          return a;
        });
        args.push(...coerced);
      }
      const out = view.querySelector(`pre[data-out="${CSS.escape(fn)}"]`) as HTMLPreElement;
      const errEl = view.querySelector(`pre[data-error="${CSS.escape(fn)}"]`) as HTMLPreElement;
      const latEl = view.querySelector(`[data-latency="${CSS.escape(fn)}"]`) as HTMLElement;
      if (errEl) errEl.textContent = '';
      if (validationError) {
        if (out) out.textContent = '';
        if (errEl) errEl.textContent = validationError;
        return;
      }
      const url = `${HOST}/plugin/${encodedName}/call`;
      const body = { function: fn, args };
      const fetchSnippet = `fetch('${url}', {\n  method: 'POST',\n  headers: { 'content-type': 'application/json' },\n  body: JSON.stringify(${JSON.stringify(body)})\n}).then(r => r.json())`;
      const curlSnippet = `curl -X POST '${url}' -H 'content-type: application/json' -d '${JSON.stringify(body)}'`;
      const fsn = view.querySelector(`pre[data-snippet-fetch="${CSS.escape(fn)}"]`);
      const csn = view.querySelector(`pre[data-snippet-curl="${CSS.escape(fn)}"]`);
      if (fsn) fsn.textContent = fetchSnippet;
      if (csn) csn.textContent = curlSnippet;
      const start = performance.now();
      try {
        const response = await fetch(url, {
          method: 'POST', headers: { 'content-type': 'application/json' },
          body: JSON.stringify(body),
        });
        const text = await response.text();
        let parsed: any = text;
        try { parsed = JSON.parse(text); } catch {}
        const ms = Math.round(performance.now() - start);
        if (latEl) latEl.textContent = `${ms} ms`;
        if (!response.ok) {
          if (out) out.textContent = '';
          let msg = `HTTP ${response.status}: ${typeof parsed === 'string' ? parsed : JSON.stringify(parsed)}`;
          if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
            const code = parsed.code ? ` [${parsed.code}]` : '';
            const message = parsed.message ? parsed.message : '';
            const hint = parsed.hint ? `\nhint: ${parsed.hint}` : '';
            const docs = parsed.docs ? `\ndocs: ${parsed.docs}` : '';
            if (message || hint || docs) {
              msg = `${code ? code.trim() + ' ' : ''}${message || msg}${hint}${docs}`.trim();
              if (!message) msg = `HTTP ${response.status}: ${msg}`;
            }
          }
          if (errEl) errEl.textContent = msg;
          return;
        }
        if (out) out.textContent = JSON.stringify(parsed, null, 2);
      } catch (e) {
        const ms = Math.round(performance.now() - start);
        if (latEl) latEl.textContent = `${ms} ms`;
        if (errEl) errEl.textContent = String(e);
      }
    });
  });
  const reloadBtn = view.querySelector('button[data-reload]');
  if (reloadBtn) {
    reloadBtn.addEventListener('click', async () => {
      const res = await fetch(`${HOST}/plugin/${encodedName}/reload`, { method: 'POST' }).then(r => r.json()).catch(e => ({ error: String(e) }));
      alert(JSON.stringify(res));
    });
  }
  const showLogs = view.querySelector('#showlogs');
  if (showLogs) {
    showLogs.addEventListener('click', async () => {
      const r = await fetch(`${HOST}/plugin/${encodedName}/logs`).then(x => x.json());
      const pre = view.querySelector('.card:last-child pre') as HTMLPreElement;
      if (pre) pre.textContent = ((r.entries || r.logs || []).map((e: any) => typeof e === 'string' ? e : `${e.ts} ${e.level} [${e.request_id}] ${e.message}`).join('\n'));
    });
  }
}

async function renderUiPlugins() {
  const plugins = await fetch(`${HOST}/ui-plugins`).then(r => r.json()).catch(() => []);
  view.innerHTML = `<h1>Webview UI plugins</h1>` + (plugins.length
    ? plugins.map((p: any) => `<div class="card"><h2><a href="#/ui/${encodeURIComponent(p.name)}">${escapeHtml(p.name)}</a></h2>
        <p>runtime: <code>js-wasm</code> · ${escapeHtml(p.language)} · ${escapeHtml(p.version)}</p></div>`).join('')
    : '<p>No UI plugins are installed.</p>');
}

async function fetchUiPluginIndex(): Promise<any[]> {
  // Prefer the Vite-served index in dev (cache-busted), fall back to host.
  for (const url of [`/ui-plugins.json?_=${Date.now()}`, `${HOST}/ui-plugins`]) {
    try {
      const r = await fetch(url, { cache: 'no-store' });
      if (!r.ok) continue;
      const data = await r.json();
      if (Array.isArray(data)) return data;
    } catch {}
  }
  return [];
}

async function renderUiPlugin(name: string) {
  if (uiPluginReloadTimer) {
    clearInterval(uiPluginReloadTimer);
    uiPluginReloadTimer = null;
  }
  const plugins = await fetchUiPluginIndex();
  const plugin = plugins.find((item: any) => item.name === name);
  if (!plugin) {
    view.textContent = `Unknown UI plugin: ${name}`;
    return;
  }
  const heading = document.createElement('h1');
  heading.textContent = `UI plugin: ${plugin.name}`;
  view.replaceChildren(heading);
  try {
    await verifyUiPluginAssets(plugin);
  } catch (error) {
    const message = document.createElement('pre');
    message.textContent = `UI plugin integrity check failed: ${String(error)}`;
    view.append(message);
    return;
  }
  const frame = document.createElement('iframe');
  frame.title = `${plugin.name} UI`;
  frame.src = `/${plugin.entry.split('/').map(encodeURIComponent).join('/')}`;
  frame.setAttribute('sandbox', 'allow-scripts allow-forms');
  frame.referrerPolicy = 'no-referrer';
  frame.style.cssText = 'width:100%;height:75vh;border:1px solid #8884;border-radius:8px';
  view.append(frame);

  if (Array.isArray(plugin.bridge) && plugin.bridge.length > 0) {
    attachBridge(frame, plugin, HOST);
  }

  let currentKey = `${plugin.version ?? ''}:${plugin.sha256 ?? ''}`;
  uiPluginReloadTimer = window.setInterval(async () => {
    const nextPlugins = await fetchUiPluginIndex();
    const next = nextPlugins.find((item: any) => item.name === name);
    if (!next) return;
    const nextKey = `${next.version ?? ''}:${next.sha256 ?? ''}`;
    if (nextKey !== currentKey) {
      currentKey = nextKey;
      frame.src = `/${next.entry.split('/').map(encodeURIComponent).join('/')}?v=${encodeURIComponent(next.sha256 ?? Date.now())}`;
    }
  }, 1500);
}

async function verifyUiPluginAssets(plugin: any) {
  if (!crypto.subtle || !plugin.files || typeof plugin.files !== 'object') {
    throw new Error('Web Crypto or signed file list unavailable');
  }
  const base = `/plugins/${encodeURIComponent(plugin.name)}/`;
  const paths = Object.keys(plugin.files).sort();
  const digestLines: string[] = [];
  for (const path of paths) {
    const response = await fetch(base + path.split('/').map(encodeURIComponent).join('/'), {
      cache: 'no-store',
    });
    if (!response.ok) throw new Error(`asset unavailable: ${path}`);
    const bytes = await response.arrayBuffer();
    const digest = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)))
      .map(byte => byte.toString(16).padStart(2, '0')).join('');
    if (digest !== plugin.files[path]) throw new Error(`asset digest mismatch: ${path}`);
    digestLines.push(`${path}\0${digest}\n`);
  }
  const aggregate = new TextEncoder().encode(digestLines.join(''));
  const digest = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', aggregate)))
    .map(byte => byte.toString(16).padStart(2, '0')).join('');
  if (digest !== plugin.sha256) throw new Error('bundle digest mismatch');
}

function renderServiceState(name: string, body: any) {
  const state = view.querySelector(`p[data-svc-state="${CSS.escape(name)}"]`) as HTMLParagraphElement | null;
  if (!state) return;
  if (body && typeof body === 'object' && 'error' in body) {
    state.textContent = `failed: ${String(body.error)}`;
    return;
  }
  const parts: string[] = [];
  if (body && typeof body === 'object' && 'stopped' in body) {
    parts.push(body.stopped ? 'stopped' : 'was not running');
  } else if (body && typeof body === 'object' && 'running' in body) {
    parts.push(body.running ? 'running' : 'stopped');
    if (body.address) parts.push(`address: ${body.address}`);
    if (body.last_error) parts.push(`last error: ${body.last_error}`);
    parts.push(`restarts: ${body.restarts ?? 0}`);
  }
  state.textContent = parts.join(' · ');
}

async function renderServices() {
  const plugins = await fetch(`${HOST}/plugins`).then(r => r.json());
  const services = plugins.filter((p: any) => p.type === 'service' || p.type === 'hybrid');
  view.innerHTML = `<h1>Services</h1>` + (services.length ? services.map((p: any) =>
    `<div class="card"><h2>${escapeHtml(p.name)}</h2>
      <button data-start="${escapeHtml(p.name)}">start</button>
      <button data-stop="${escapeHtml(p.name)}">stop</button>
      <button data-restart="${escapeHtml(p.name)}">restart</button>
      <button data-status="${escapeHtml(p.name)}">status</button>
      <p data-svc-state="${escapeHtml(p.name)}">unknown</p>
      <pre data-svc="${escapeHtml(p.name)}"></pre></div>`).join('') : '<p>No service plugins.</p>');
  const updateService = async (name: string, action: 'start' | 'stop' | 'restart' | 'status') => {
    const encoded = encodeURIComponent(name);
    const method = action === 'status' ? 'GET' : 'POST';
    const res = await fetch(`${HOST}/plugin/${encoded}/service/${action}`, {
      method,
      ...(action === 'start' ? {
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ region_json: '' }),
      } : {}),
    }).then(async response => {
      const body = await response.json().catch(() => response.text());
      if (!response.ok) throw new Error(typeof body === 'string' ? body : JSON.stringify(body));
      return body;
    }).catch(error => ({ error: String(error) }));
    const output = view.querySelector(`pre[data-svc="${CSS.escape(name)}"]`) as HTMLPreElement;
    if (output) output.textContent = JSON.stringify(res, null, 2);
    renderServiceState(name, res);
  };
  for (const action of ['start', 'stop', 'restart', 'status'] as const) {
    view.querySelectorAll(`button[data-${action}]`).forEach(button => {
      button.addEventListener('click', () => {
        const name = (button as HTMLElement).getAttribute(`data-${action}`)!;
        void updateService(name, action);
      });
    });
  }
  // Show current lifecycle state (including a previous failure reason) on load.
  for (const service of services) {
    void updateService(service.name, 'status');
  }
}

async function renderDev() {
  const [status, plugins] = await Promise.all([
    fetch(`${HOST}/dev/status`).then(response => response.json()).catch(() => ({ active: false, watch_paths: [], last_reload: {} })),
    fetch(`${HOST}/plugins`).then(response => response.json()).catch(() => []),
  ]);
  const watched = status.watch_paths?.length
    ? status.watch_paths.map((path: string) => `<li><code>${escapeHtml(path)}</code></li>`).join('')
    : '<li>none</li>';
  const reloadRows = plugins.map((plugin: any) => {
    const record = status.last_reload?.[plugin.name];
    const result = record
      ? `${record.ok ? 'ok' : 'failed'} (HTTP ${record.http_status}) · ${new Date(record.timestamp_unix * 1000).toLocaleTimeString()}`
      : 'not reloaded yet';
    const button = plugin.runtime === 'wasi-component'
      ? `<button data-dev-reload="${escapeHtml(plugin.name)}">reload</button>`
      : '';
    return `<tr><td>${escapeHtml(plugin.name)}</td><td>${escapeHtml(result)}</td><td>${button}</td></tr>`;
  }).join('');
  view.innerHTML = `<h1>Dev mode</h1>
    <div class="card">
      <p>Watcher: <strong>${status.active ? 'active' : 'inactive'}</strong> · Hot reload: <strong>${status.hot_reload ? 'enabled' : 'disabled'}</strong></p>
      <p>Use <code>owasidea dev</code> to start the host, UI server, watcher, and reload controls.</p>
      <h2>Watch paths</h2><ul>${watched}</ul>
    </div>
    <div class="card"><h2>Plugin reload status</h2>
      <table><thead><tr><th>Plugin</th><th>Last result</th><th>Action</th></tr></thead><tbody>${reloadRows}</tbody></table>
    </div>`;
  view.querySelectorAll('button[data-dev-reload]').forEach(button => {
    button.addEventListener('click', async () => {
      const name = (button as HTMLElement).getAttribute('data-dev-reload')!;
      const response = await fetch(`${HOST}/plugin/${encodeURIComponent(name)}/reload`, { method: 'POST' });
      if (!response.ok) {
        const message = await response.text();
        alert(`Reload failed (${response.status}): ${message}`);
      }
      await renderDev();
    });
  });
}

window.addEventListener('hashchange', route);
route();
