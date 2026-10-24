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
    const card = document.createElement('div');
    card.className = 'card';
    card.innerHTML = `<h2>${p.name} <small>(${p.language}, ${p.type}, ${p.runtime ?? 'wasi-component'})</small></h2>
      <p>exports: ${p.exports.join(', ')}</p>
      <button data-greet="${p.name}">greet</button>
      <pre id="out-${p.name}"></pre>`;
    pluginsDiv.appendChild(card);
  }
  pluginsDiv.addEventListener('click', async (e) => {
    const t = e.target as HTMLElement;
    const name = t.getAttribute('data-greet');
    if (!name) return;
    const out = document.getElementById(`out-${name}`)!;
    const res = await fetch(`${HOST}/plugin/${name}/greet`, {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ name: 'UI' }),
    }).then(r => r.json());
    out.textContent = JSON.stringify(res);
  });
}

main();