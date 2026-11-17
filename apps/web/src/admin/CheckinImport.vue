<template>
  <section class="stack" aria-labelledby="checkin-import">
    <h2 id="checkin-import">{{ t('checkin.import.heading') }}</h2>
    <label for="checkin-import-file">{{ t('checkin.import.file') }}</label>
    <input id="checkin-import-file" ref="input" type="file" accept=".json,application/json" @change="choose" />
    <p v-if="error" role="alert">{{ error }}</p>
    <template v-if="preview">
      <p role="status">
        {{ t('checkin.import.summary', { added: preview.added, updated: preview.updated, skipped: preview.skipped.length }) }}
      </p>
      <ul v-if="preview.skipped.length" class="skipped">
        <li v-for="(s, i) in preview.skipped" :key="i">{{ s.key }}：{{ t(`checkin.import.reasons.${s.reason}`) }}</li>
      </ul>
      <Button :label="t('checkin.import.confirm')" icon="pi pi-upload" :disabled="busy" @click="apply" />
    </template>
  </section>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { successToast } from './home'
import { keepFocus } from './page'

// The file is only read here; its content is posted for a preview, then again to import it.
const MAX_BYTES = 5 * 1024 * 1024
const { t } = useI18n()
const toast = useToast()
const emit = defineEmits<{ imported: [] }>()
const input = ref<HTMLInputElement>()
const preview = shallowRef<Schemas['ImportSummary']>()
const error = ref('')
const busy = ref(false)
// The OpenAPI body is a free-form object; the Host checks every entry.
let content: Record<string, never> | undefined

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))

/** Shows why the file was refused and clears the choice, so choosing the same file once fixed fires again. */
function refuse(text: string) {
  error.value = text
  content = undefined
  if (input.value) input.value.value = ''
}

async function choose() {
  preview.value = undefined
  error.value = ''
  content = undefined
  const file = input.value?.files?.[0]
  if (!file) return
  if (file.size > MAX_BYTES) return refuse(t('errors.checkin.import_too_large'))
  try {
    content = JSON.parse(await file.text())
  } catch {
    return refuse(t('errors.checkin.import_invalid'))
  }
  try {
    preview.value = await api(client.POST('/api/admin/checkin/import/preview', { body: content! }))
  } catch (e) {
    refuse(message(e))
  }
}

async function apply() {
  if (!content) return
  busy.value = true
  try {
    const result = await api(client.POST('/api/admin/checkin/import', { body: content }))
    toast.add(successToast(t('checkin.import.done', { added: result.added, updated: result.updated })))
    preview.value = undefined
    content = undefined
    if (input.value) input.value.value = ''
    emit('imported')
    // The confirm button is gone with the preview.
    void keepFocus(() => input.value)
  } catch (e) {
    error.value = message(e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.skipped {
  margin: 0;
  overflow-wrap: anywhere;
}

h2 {
  margin-bottom: 0;
}
</style>
