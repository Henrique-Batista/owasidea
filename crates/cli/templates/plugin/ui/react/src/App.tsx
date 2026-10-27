export default function App() {
  const host = (window as any).__OWASIDEA_HOST__ || '';
  return (
    <main>
      <h1>{{name}}</h1>
      <p>OWASIDEA host: {host || '(none)'}</p>
    </main>
  );
}
