const HOST = 'http://127.0.0.1:8787';

async function main() {
  const status = document.getElementById('status')!;
  const pluginsDiv = document.getElementById('plugins')!;
  try {
    const health = await fetch(`${HOST}/health`).then(r => r.text());
    status.textContent = `host: ${health}`;
  } catch (e) {
    status.textContent = `host unreachable: ${e}`;
    return;
  }
  const plugins = await fetch(`${HOST}/plugins`).then(r => r.json());
  for (const p of plugins) {
    const manifest = await fetch(`${HOST}/plugin/${p.name}/manifest`).then(r => r.json());
    const card = document.createElement('div');
    card.className = 'card';
    card.innerHTML = `<h2>${p.name} <small>(${p.language}, ${p.type})</small></h2>
      <p>exports: ${p.exports.join(', ')}</p>`;
    for (const exp of manifest.exports || []) {
      const row = document.createElement('div');
      row.style.margin = '0.5rem 0';
      row.innerHTML = `<strong>${exp.name}</strong> <code>${exp.signature}</code><br/>
        <input type="text" placeholder="args (comma separated)" style="width:60%" />
        <button>call</button>
        <pre style="background:#f5f5f5;padding:0.5rem"></pre>`;
      const input = row.querySelector('input') as HTMLInputElement;
      const btn = row.querySelector('button') as HTMLButtonElement;
      const out = row.querySelector('pre') as HTMLPreElement;
      btn.addEventListener('click', async () => {
        const args = input.value.trim() === '' ? [] : input.value.split(',').map(s => s.trim());
        // try to coerce numbers/booleans
        const coerced = args.map(a => {
          if (a === 'true') return true;
          if (a === 'false') return false;
          if (/^-?\d+$/.test(a)) return Number(a);
          return a;
        });
        const res = await fetch(`${HOST}/plugin/${p.name}/call`, {
          method: 'POST', headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ function: exp.name, args: coerced }),
        }).then(r => r.json());
        out.textContent = JSON.stringify(res, null, 2);
      });
      card.appendChild(row);
    }
    // logs
    const logsBtn = document.createElement('button');
    logsBtn.textContent = 'show logs';
    const logsPre = document.createElement('pre');
    logsBtn.addEventListener('click', async () => {
      const r = await fetch(`${HOST}/plugin/${p.name}/logs`).then(x => x.json());
      logsPre.textContent = (r.logs || []).join('\n');
    });
    card.appendChild(logsBtn);
    card.appendChild(logsPre);
    pluginsDiv.appendChild(card);
  }
}

main();