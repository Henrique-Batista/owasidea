<script lang="ts">
  const host = (window as any).__OWASIDEA_HOST__ || '';
</script>

<main>
  <h1>{{name}}</h1>
  <p>OWASIDEA host: {host || '(none)'}</p>
</main>
