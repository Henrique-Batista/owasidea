import { useMemo, useState } from 'react';

type Node = {
  id: string;
  language: string;
  role: string;
  color: string;
  detail: string;
  x: number;
  y: number;
};

const nodes: Node[] = [
  { id: 'rust', language: 'Rust', role: 'readiness core + service', color: '#f97316', detail: 'Scores the release, persists reports through host KV and a WASI preopen, and runs the persistent watchtower lifecycle.', x: 50, y: 30 },
  { id: 'go', language: 'Go', role: 'audit + composed pipeline + HTTP', color: '#22d3ee', detail: 'Audits dependencies, imports the Rust readiness interface at build time through wasm-tools compose, and serves the webhook via wasi:http.', x: 20, y: 68 },
  { id: 'typescript', language: 'TypeScript', role: 'changelog renderer', color: '#60a5fa', detail: 'Bundled with jco/componentize-js. Groups release notes into sections inside a WASI component.', x: 47, y: 82 },
  { id: 'csharp', language: 'C#', role: 'policy gate', color: '#a78bfa', detail: 'componentize-dotnet library that classifies the score and calculates the next semantic version.', x: 77, y: 71 },
  { id: 'c', language: 'C', role: 'fingerprint', color: '#facc15', detail: 'wasi-sdk + wit-bindgen component that produces a deterministic FNV-1a release fingerprint.', x: 83, y: 28 },
  { id: 'cpp', language: 'C++', role: 'text metrics', color: '#f472b6', detail: 'C++20 component that analyzes the generated changelog for words, lines, and reading time.', x: 65, y: 12 },
];

const edges: Array<[string, string, string]> = [
  ['rust', 'go', 'composed'],
  ['rust', 'csharp', 'score'],
  ['typescript', 'cpp', 'markdown'],
  ['go', 'typescript', 'ui orchestration'],
  ['c', 'rust', 'fingerprint'],
];

export default function App() {
  const [selected, setSelected] = useState(nodes[0]);
  const lines = useMemo(() => edges.map(([from, to, label]) => {
    const a = nodes.find(node => node.id === from)!;
    const b = nodes.find(node => node.id === to)!;
    return { key: `${from}-${to}`, x1: a.x, y1: a.y, x2: b.x, y2: b.y, label };
  }), []);

  return (
    <div className="shell">
      <header>
        <p className="eyebrow">isolated UI · React 18</p>
        <h1>Polyglot architecture map</h1>
        <p className="lede">Six runtimes, one WIT contract boundary. Click a language node to inspect how it participates in the release flow.</p>
      </header>

      <div className="layout">
        <div className="map" role="img" aria-label="Shipyard plugin architecture map">
          <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
            {lines.map(line => (
              <line key={line.key} x1={line.x1} y1={line.y1} x2={line.x2} y2={line.y2} />
            ))}
          </svg>
          {nodes.map(node => (
            <button
              key={node.id}
              className={`node ${selected.id === node.id ? 'active' : ''}`}
              style={{ left: `${node.x}%`, top: `${node.y}%`, '--node': node.color } as React.CSSProperties}
              onClick={() => setSelected(node)}
              aria-pressed={selected.id === node.id}
            >
              <strong>{node.language}</strong>
              <span>{node.role}</span>
            </button>
          ))}
        </div>

        <aside style={{ '--node': selected.color } as React.CSSProperties}>
          <p className="eyebrow">selected runtime</p>
          <h2>{selected.language}</h2>
          <p className="role">{selected.role}</p>
          <p>{selected.detail}</p>
          <div className="contract">
            <span>WIT boundary</span>
            <code>shipyard:*/api@0.1.0</code>
          </div>
        </aside>
      </div>

      <footer>
        <span className="pulse" />
        React bundle loaded from an isolated iframe; no host bridge.
      </footer>
    </div>
  );
}
