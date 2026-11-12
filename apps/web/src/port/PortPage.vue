<template>
  <main>
    <h1>連接埠被佔用</h1>
    <p>Host 無法使用目前的連接埠。請輸入新的連接埠，OBS 裡的網址也要一起更新。</p>
    <form @submit.prevent="retry">
      <label for="port">連接埠</label>
      <input id="port" v-model.number="port" type="number" min="1024" max="65535" required />
      <button type="submit" :disabled="busy">儲存並重試</button>
    </form>
    <p v-if="error" role="alert">{{ error }}</p>
  </main>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'

// Error codes come from the `retry_with_port` command; messages move to vue-i18n with the skeleton foundations.
const messages: Record<string, string> = {
  invalid_port: '連接埠必須介於 1024 到 65535。',
  port_in_use: '這個連接埠也被佔用了，請換一個。',
}

const port = ref(47811)
const busy = ref(false)
const error = ref('')

async function retry() {
  busy.value = true
  error.value = ''
  try {
    await invoke('retry_with_port', { port: port.value })
  } catch (code) {
    error.value = messages[String(code)] ?? '無法啟動 Host，請查看日誌。'
  } finally {
    busy.value = false
  }
}
</script>
