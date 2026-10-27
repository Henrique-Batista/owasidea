declare const __OWASIDEA_HOST__: string;

const HOST = __OWASIDEA_HOST__;

async function call<T>(fn: string, args: unknown[]): Promise<T> {
  const response = await fetch(`${HOST}/plugin/greeter/call`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ function: fn, args }),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`${response.status}: ${text}`);
  }
  return (JSON.parse(text) as { result: T }).result;
}

function show(target: HTMLElement, text: string, failed = false) {
  target.textContent = text;
  target.classList.toggle('error', failed);
}

document.getElementById('greet-form')!.addEventListener('submit', async event => {
  event.preventDefault();
  const name = (document.getElementById('name') as HTMLInputElement).value;
  const out = document.getElementById('greeting')!;
  try {
    show(out, await call<string>('greet', [name]));
  } catch (error) {
    show(out, String(error), true);
  }
});

document.getElementById('sum-form')!.addEventListener('submit', async event => {
  event.preventDefault();
  const a = Number((document.getElementById('a') as HTMLInputElement).value);
  const b = Number((document.getElementById('b') as HTMLInputElement).value);
  const out = document.getElementById('total')!;
  try {
    show(out, String(await call<number>('sum', [a, b])));
  } catch (error) {
    show(out, String(error), true);
  }
});
