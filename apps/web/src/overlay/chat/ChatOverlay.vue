<template>
  <div class="chat">
    <p v-if="status !== 'open'" class="status">{{ status }}</p>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'

const status = ref<'connecting' | 'open' | 'closed'>('connecting')
const ws = new WebSocket(`ws://${location.host}/ws/overlay`)
ws.onopen = () => (status.value = 'open')
ws.onclose = () => (status.value = 'closed')
onBeforeUnmount(() => ws.close())
</script>
