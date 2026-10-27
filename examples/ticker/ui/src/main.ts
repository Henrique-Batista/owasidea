declare const __OWASIDEA_HOST__: string;

const HOST = __OWASIDEA_HOST__;
const PLUGIN = 'ticker';
const statusEl = document.querySelector<HTMLElement>('[data-testid="status"]')!;
const messageEl = document.getElementById('message')!;

function say(text: string, failed = false) {
  messageEl.textContent = text;
  messageEl.classList.toggle('error', failed);
}

async function refresh() {
  try {
    const response = await fetch(`${HOST}/plugin/${PLUGIN}/service/status`);
    const body = (await response.json()) as { running: boolean };
    statusEl.textContent = body.running ? 'running' : 'stopped';
  } catch {
    statusEl.textContent = 'host unreachable';
  }
}

async function post(path: string, body?: unknown): Promise<string> {
  const response = await fetch(`${HOST}/plugin/${PLUGIN}/${path}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`${response.status}: ${text}`);
  }
  return text;
}

async function act(label: string, path: string, body?: unknown) {
  try {
    await post(path, body);
    say(`${label} ok`);
  } catch (error) {
    say(String(error), true);
  } finally {
    await refresh();
  }
}

document.getElementById('start')!.addEventListener('click', () =>
  act('start', 'service/start', { region_json: '' }),
);
document.getElementById('stop')!.addEventListener('click', () => act('stop', 'service/stop'));
document.getElementById('reload')!.addEventListener('click', () => act('reload', 'reload'));

void refresh();
setInterval(refresh, 1000);
