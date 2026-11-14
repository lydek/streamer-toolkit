<template>
  <RouterView />
  <CloseHint />
  <!-- Fits the window below 600px instead of the default 25rem width. -->
  <Toast :breakpoints="{ '600px': { width: 'calc(100vw - 2.5rem)' } }" />
</template>

<script setup lang="ts">
import Toast from 'primevue/toast'
import CloseHint from './CloseHint.vue'
</script>
