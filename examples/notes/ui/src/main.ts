declare const __OWASIDEA_HOST__: string;

const HOST = __OWASIDEA_HOST__;
const list = document.getElementById('notes')!;
const count = document.querySelector<HTMLElement>('[data-testid="count"]')!;
const errorBox = document.getElementById('error')!;

async function call<T>(fn: string, args: unknown[]): Promise<T> {
  const response = await fetch(`${HOST}/plugin/notes/call`, {
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

function render(notes: string[]) {
  list.replaceChildren(
    ...notes.map(note => {
      const item = document.createElement('li');
      item.textContent = note;
      return item;
    }),
  );
  count.textContent = `${notes.length} ${notes.length === 1 ? 'note' : 'notes'}`;
}

async function refresh() {
  const raw = await call<string>('list-notes', []);
  render(raw ? raw.split('\n') : []);
}

async function run(action: () => Promise<unknown>) {
  errorBox.textContent = '';
  try {
    await action();
    await refresh();
  } catch (error) {
    errorBox.textContent = String(error);
  }
}

document.getElementById('add-form')!.addEventListener('submit', event => {
  event.preventDefault();
  const input = document.getElementById('text') as HTMLInputElement;
  const text = input.value;
  input.value = '';
  void run(() => call<number>('add-note', [text]));
});

document.getElementById('clear')!.addEventListener('click', () => {
  void run(() => call<number>('clear-notes', []));
});

void run(async () => {});
