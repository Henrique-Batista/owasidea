const HOST = 'http://127.0.0.1:8787';
const view = document.getElementById('view')!;

function route() {
  const hash = location.hash || '#/overview';
  if (hash.startsWith('#/plugins/')) {
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
  view.innerHTML = `<h1>Overview</h1>
    <div class="card">
      <p>host: ${health}</p>
      <p>plugins: ${ov.plugins ?? 0} (libraries: ${ov.libraries ?? 0}, executables: ${ov.executables ?? 0}, services: ${ov.services ?? 0})</p>
      <p>runtimes: ${(ov.runtimes || []).join(', ')}</p>
    </div>`;
}

async function renderPlugins() {
  const plugins = await fetch(`${HOST}/plugins`).then(r => r.json());
  view.innerHTML = `<h1>Plugins</h1>` + plugins.map((p: any) =>
    `<div class="card"><h2><a href="#/plugins/${encodeURIComponent(p.name)}">${p.name}</a> <small>(${p.language}, ${p.type})</small></h2>
     <p>exports: ${p.exports.join(', ')}</p></div>`).join('');
}

async function renderPluginDetail(name: string) {
  const manifest = await fetch(`${HOST}/plugin/${name}/manifest`).then(r => r.json());
  const logs = await fetch(`${HOST}/plugin/${name}/logs`).then(r => r.json()).catch(() => ({ logs: [] }));
  let html = `<h1>Plugin: ${name}</h1>
    <div class="card"><h2>Manifest</h2><pre>${JSON.stringify(manifest, null, 2)}</pre></div>
    <div class="card"><h2>Exports</h2>`;
  for (const exp of manifest.exports || []) {
    html += `<div style="margin:0.5rem 0"><strong>${exp.name}</strong> <code>${exp.signature}</code><br/>
      <input type="text" placeholder="args comma separated" data-input="${exp.name}" style="width:60%" />
      <button data-call="${exp.name}">call</button>
      <button data-reload="1">reload</button>
      <pre data-out="${exp.name}"></pre></div>`;
  }
  html += `</div>
    <div class="card"><h2>Logs</h2><pre>${(logs.logs || []).join('\n')}</pre>
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
      const res = await fetch(`${HOST}/plugin/${name}/call`, {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ function: fn, args: coerced }),
      }).then(r => r.json());
      out.textContent = JSON.stringify(res, null, 2);
    });
  });
  const reloadBtn = view.querySelector('button[data-reload]');
  if (reloadBtn) {
    reloadBtn.addEventListener('click', async () => {
      const res = await fetch(`${HOST}/plugin/${name}/reload`, { method: 'POST' }).then(r => r.json()).catch(e => ({ error: String(e) }));
      alert(JSON.stringify(res));
    });
  }
  const showLogs = view.querySelector('#showlogs');
  if (showLogs) {
    showLogs.addEventListener('click', async () => {
      const r = await fetch(`${HOST}/plugin/${name}/logs`).then(x => x.json());
      const pre = view.querySelector('.card:last-child pre') as HTMLPreElement;
      if (pre) pre.textContent = (r.logs || []).join('\n');
    });
  }
}

async function renderServices() {
  const plugins = await fetch(`${HOST}/plugins`).then(r => r.json());
  const services = plugins.filter((p: any) => p.type === 'hybrid' || p.type === 'executable');
  view.innerHTML = `<h1>Services</h1>` + (services.length ? services.map((p: any) =>
    `<div class="card"><h2>${p.name}</h2>
      <button data-start="${p.name}">start</button>
      <button data-stop="${p.name}">stop</button>
      <button data-status="${p.name}">status</button>
      <pre data-svc="${p.name}"></pre></div>`).join('') : '<p>No service plugins.</p>');
  view.querySelectorAll('button[data-start]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const n = (btn as HTMLElement).getAttribute('data-start')!;
      const res = await fetch(`${HOST}/plugin/${n}/start-service` ?? `${HOST}/plugin/tailcat-wasi/start-service`, {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ region_json: '' }),
      }).then(r => r.json()).catch(e => ({ error: String(e) }));
      const pre = view.querySelector(`pre[data-svc="${n}"]`) as HTMLPreElement;
      if (pre) pre.textContent = JSON.stringify(res);
    });
  });
}

async function renderDev() {
  view.innerHTML = `<h1>Dev mode</h1>
    <div class="card">
      <p>Watched files and reload status are reported by the host logs.</p>
      <p>Use <code>owasidea dev</code> to start host + UI dev server.</p>
    </div>`;
}

window.addEventListener('hashchange', route);
route();