<script setup lang="ts">
const host = (window as any).__OWASIDEA_HOST__ || '';
</script>

<template>
  <main>
    <h1>{{name}}</h1>
    <p>OWASIDEA host: {{ host || '(none)' }}</p>
  </main>
</template>
