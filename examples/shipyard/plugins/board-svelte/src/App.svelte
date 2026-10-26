<script lang="ts">
  type Column = 'ready' | 'review' | 'blocked';
  type Item = { id: number; label: string; column: Column };

  const columns: Array<{ key: Column; title: string; hint: string }> = [
    { key: 'ready', title: 'Ready', hint: 'safe to ship' },
    { key: 'review', title: 'Review', hint: 'needs an owner' },
    { key: 'blocked', title: 'Blocked', hint: 'do not ship' },
  ];

  const defaults: Item[] = [
    { id: 1, label: 'Rust readiness score', column: 'ready' },
    { id: 2, label: 'Go dependency audit', column: 'ready' },
    { id: 3, label: 'C# policy gate', column: 'review' },
    { id: 4, label: 'C++ docs metrics', column: 'review' },
    { id: 5, label: 'Webhook smoke test', column: 'blocked' },
  ];

  let items: Item[] = load();
  let draft = '';

  function load(): Item[] {
    try {
      const raw = localStorage.getItem('shipyard-board');
      if (raw) return JSON.parse(raw);
    } catch {
      // ignore isolated storage failures
    }
    return defaults;
  }

  function persist(next: Item[]) {
    items = next;
    try {
      localStorage.setItem('shipyard-board', JSON.stringify(next));
    } catch {
      // sandboxed storage is best effort
    }
  }

  function move(id: number) {
    const order: Column[] = ['ready', 'review', 'blocked'];
    persist(items.map(item => item.id === id
      ? { ...item, column: order[(order.indexOf(item.column) + 1) % order.length] }
      : item));
  }

  function add() {
    const label = draft.trim();
    if (!label) return;
    persist([...items, { id: Date.now(), label, column: 'review' }]);
    draft = '';
  }

  function reset() {
    persist(defaults);
  }

  function count(column: Column) {
    return items.filter(item => item.column === column).length;
  }
</script>

<div class="shell">
  <header>
    <div>
      <p class="eyebrow">isolated UI · Svelte 4</p>
      <h1>Release train board</h1>
      <p class="lede">This micro-frontend runs in its own bundle and iframe. It shares no JavaScript state with the host dashboard.</p>
    </div>
    <button class="ghost" on:click={reset}>reset board</button>
  </header>

  <form class="composer" on:submit|preventDefault={add}>
    <input bind:value={draft} placeholder="Add a release check…" aria-label="New release check" />
    <button type="submit">add</button>
  </form>

  <div class="columns">
    {#each columns as column}
      <section class="column {column.key}">
        <div class="column-head">
          <div>
            <h2>{column.title}</h2>
            <p>{column.hint}</p>
          </div>
          <span class="count">{count(column.key)}</span>
        </div>
        <div class="items">
          {#each items.filter(item => item.column === column.key) as item (item.id)}
            <button class="item" on:click={() => move(item.id)} title="Move to the next column">
              {item.label}
              <span>→</span>
            </button>
          {/each}
          {#if count(column.key) === 0}
            <p class="empty">nothing here</p>
          {/if}
        </div>
      </section>
    {/each}
  </div>

  <footer>
    <span class="dot"></span>
    State lives in this iframe's localStorage only.
  </footer>
</div>
