<template>
  <FullPage :title="t('modules.names.sound')">
    <div class="stack">
      <label for="sound-file">{{ t('sound.file') }}</label>
      <input
        id="sound-file"
        ref="input"
        type="file"
        accept=".mp3,.wav,.ogg,audio/mpeg,audio/wav,audio/ogg"
        :disabled="busy"
        @change="choose"
      />
      <p v-if="importError" role="alert">{{ importError }}</p>
    </div>

    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!library">{{ t('app.loading') }}</p>
    <p v-else-if="library.sounds.length === 0">{{ t('sound.empty') }}</p>
    <ul v-else class="sounds">
      <SoundRow
        v-for="s in library.sounds"
        :key="s.id"
        :sound="s"
        :used-by="s.used_by.map((slot) => slotLabel(slot, t))"
        @deleted="deleted"
        @error="report"
      />
    </ul>
  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'
import { importSound, slotLabel } from './sounds'
import SoundRow from './SoundRow.vue'

const { t } = useI18n()
const toast = useToast()
const input = ref<HTMLInputElement>()
const library = shallowRef<Schemas['SoundLibrary']>()
const failed = ref('')
const importError = ref('')
const busy = ref(false)

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))

async function load() {
  try {
    library.value = await api(client.GET('/api/admin/sounds'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : t('sound.loadFailed')
  }
}

async function choose() {
  importError.value = ''
  const file = input.value?.files?.[0]
  if (!file) return
  busy.value = true
  try {
    const sound = await importSound(file)
    toast.add(successToast(t('sound.imported', { label: sound.label })))
    await load()
  } catch (e) {
    importError.value = message(e)
  } finally {
    busy.value = false
    if (input.value) input.value.value = ''
  }
}

function deleted(s: Schemas['Sound']) {
  toast.add(successToast(t('sound.deleted', { label: s.label })))
  void load()
}

const report = (e: unknown) => toast.add(errorToast(message(e)))

void load()
</script>

<style scoped>
.sounds {
  list-style: none;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
</style>
