<template>
  <main>
    <h1>{{ t('port.heading') }}</h1>
    <p>{{ t('port.explain') }}</p>
    <form @submit.prevent="retry">
      <label for="port">{{ t('port.label') }}</label>
      <input id="port" v-model.number="port" type="number" min="1024" max="65535" required />
      <button type="submit" :disabled="busy">{{ t('port.retry') }}</button>
    </form>
    <p v-if="error" role="alert">{{ error }}</p>
  </main>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'

const { t } = useI18n()

const port = ref(47811)
const busy = ref(false)
const error = ref('')

async function retry() {
  busy.value = true
  error.value = ''
  try {
    await invoke('retry_with_port', { port: port.value })
  } catch (code) {
    // Error codes come from the `retry_with_port` command.
    error.value = errorMessage(String(code))
  } finally {
    busy.value = false
  }
}
</script>
