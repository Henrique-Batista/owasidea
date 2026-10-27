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
  const logs = await fetch(`${HOST}/plugin/${encodedName}/logs`).then(r => r.json()).catch(() => ({ logs: [] }));
  let html = `<h1>Plugin: ${escapeHtml(name)}</h1>
    <div class="card"><h2>Manifest</h2><pre>${escapeHtml(JSON.stringify(manifest, null, 2))}</pre></div>
    <div class="card"><h2>Exports</h2>`;
  for (const exp of manifest.exports || []) {
    html += `<div style="margin:0.5rem 0"><strong>${escapeHtml(exp.name)}</strong> <code>${escapeHtml(exp.signature)}</code><br/>
      <input type="text" placeholder="args comma separated" data-input="${escapeHtml(exp.name)}" style="width:60%" />
      <button data-call="${escapeHtml(exp.name)}">call</button>
      <button data-reload="1">reload</button>
      <pre data-out="${escapeHtml(exp.name)}"></pre></div>`;
  }
  html += `</div>
    <div class="card"><h2>Logs</h2><pre>${escapeHtml((logs.logs || []).join('\n'))}</pre>
      <button id="showlogs">refresh logs</button></div>`;
  view.innerHTML = html;

  view.querySelectorAll('button[data-call]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const fn = (btn as HTMLElement).getAttribute('data-call')!;
      const input = view.querySelector(`input[data-input="${fn}"]`) as HTMLInputElement;
      const out = view.querySelector(`pre[data-out="${fn}"]`) as HTMLPreElement;
      const args = input.value.trim() === '' ? [] : input.value.split(',').map(s => s.trim());
      const coerced = args.map(a => {
        if (a === 'true') return true;
        if (a === 'false') return false;
        if (/^-?\d+$/.test(a)) return Number(a);
        return a;
      });
      const res = await fetch(`${HOST}/plugin/${encodedName}/call`, {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ function: fn, args: coerced }),
      }).then(r => r.json());
      out.textContent = JSON.stringify(res, null, 2);
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
      if (pre) pre.textContent = (r.logs || []).join('\n');
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

async function renderUiPlugin(name: string) {
  const plugins = await fetch(`${HOST}/ui-plugins`).then(r => r.json());
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

async function renderServices() {
  const plugins = await fetch(`${HOST}/plugins`).then(r => r.json());
  const services = plugins.filter((p: any) => p.type === 'service' || p.type === 'hybrid');
  view.innerHTML = `<h1>Services</h1>` + (services.length ? services.map((p: any) =>
    `<div class="card"><h2>${escapeHtml(p.name)}</h2>
      <button data-start="${escapeHtml(p.name)}">start</button>
      <button data-stop="${escapeHtml(p.name)}">stop</button>
      <button data-status="${escapeHtml(p.name)}">status</button>
      <pre data-svc="${escapeHtml(p.name)}"></pre></div>`).join('') : '<p>No service plugins.</p>');
  const updateService = async (name: string, action: 'start' | 'stop' | 'status') => {
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
      if (!response.ok) throw new Error(JSON.stringify(body));
      return body;
    }).catch(error => ({ error: String(error) }));
    const output = view.querySelector(`pre[data-svc="${CSS.escape(name)}"]`) as HTMLPreElement;
    if (output) output.textContent = JSON.stringify(res, null, 2);
  };
  for (const action of ['start', 'stop', 'status'] as const) {
    view.querySelectorAll(`button[data-${action}]`).forEach(button => {
      button.addEventListener('click', () => {
        const name = (button as HTMLElement).getAttribute(`data-${action}`)!;
        void updateService(name, action);
      });
    });
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
