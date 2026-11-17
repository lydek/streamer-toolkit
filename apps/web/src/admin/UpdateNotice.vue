<template>
  <section v-if="available" class="update" aria-labelledby="update-heading">
    <h2 id="update-heading">{{ t('update.available', { version: available }) }}</h2>
    <p>{{ t('update.current', { version: current }) }}</p>
    <div class="actions">
      <Button :label="t('update.download')" icon="pi pi-external-link" :disabled="busy" @click="download" />
      <Button :label="t('update.dismiss')" severity="secondary" outlined :disabled="busy" @click="dismiss" />
    </div>
  </section>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, api, client } from './api'
import { errorToast } from './home'

// The Host checks GitHub (at start, then every 24 h); this only asks the Host what to show.
const POLL_MS = 60_000

const { t } = useI18n()
const toast = useToast()
const emit = defineEmits<{ dismissed: [] }>()
const available = ref<string | null>(null)
const current = ref('')
const busy = ref(false)
let timer: number | undefined

// A failed read just keeps the notice as it is: no error for a notice.
async function load() {
  const state = await api(client.GET('/api/admin/update')).catch(() => undefined)
  if (!state) return
  available.value = state.available ?? null
  current.value = state.current
}

async function act(action: () => Promise<unknown>) {
  busy.value = true
  try {
    await action()
  } catch (e) {
    toast.add(errorToast(errorMessage(e instanceof ApiError ? e.code : String(e))))
  } finally {
    busy.value = false
  }
}

// The command opens a fixed releases page; it takes no URL.
const download = () => act(() => invoke('open_download_page'))
const dismiss = () =>
  act(async () => {
    await api(client.PUT('/api/admin/update/dismiss', { body: { version: available.value! } }))
    available.value = null
    // The notice, with the focused button, is gone: the page moves focus on.
    emit('dismissed')
  })

onMounted(() => {
  void load()
  timer = window.setInterval(() => void load(), POLL_MS)
})
onBeforeUnmount(() => window.clearInterval(timer))
</script>

<style scoped>
.update {
  border: 1px solid var(--p-primary-color);
  border-radius: 6px;
  padding: 0 1rem 1rem;
  margin-bottom: 1rem;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
</style>
