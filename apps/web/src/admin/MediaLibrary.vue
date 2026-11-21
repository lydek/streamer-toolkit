<template>
  <div class="stack">
    <label for="media-file">{{ t('screenTools.media.file') }}</label>
    <input id="media-file" ref="input" type="file" multiple :accept="MEDIA_ACCEPT" :disabled="busy" @change="choose" />
    <ul v-if="importErrors.length" role="alert" class="errors">
      <li v-for="e in importErrors" :key="e">{{ e }}</li>
    </ul>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!items">{{ t('app.loading') }}</p>
    <p v-else-if="!items.length">{{ t('screenTools.media.empty') }}</p>
    <table v-else class="table">
      <thead>
        <tr>
          <th scope="col">{{ t('screenTools.media.title') }}</th>
          <th scope="col">{{ t('screenTools.media.length') }}</th>
          <th scope="col"><span class="sr-only">{{ t('screenTools.media.actions') }}</span></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="item in items" :key="item.id" :data-media="item.id">
          <td>
            {{ item.title }}
            <span class="muted">{{ item.format.toUpperCase() }}</span>
            <span v-if="item.missing" class="muted">{{ t('screenTools.media.missing') }}</span>
          </td>
          <td>
            <span v-if="item.format === 'gif'" class="seconds">
              <InputNumber
                :model-value="(item.duration_ms ?? 5000) / 1000"
                :min="1"
                :max="600"
                :max-fraction-digits="1"
                :aria-label="t('screenTools.media.seconds', { title: item.title })"
                @update:model-value="(v: number | null) => setSeconds(item, v)"
              />
              {{ t('screenTools.media.secondsUnit') }}
            </span>
            <span v-else>{{ item.duration_ms == null ? t('screenTools.media.reading') : formatLength(item.duration_ms) }}</span>
          </td>
          <td class="end">
            <span v-if="confirming === item.id" role="group" :aria-label="t('screenTools.media.delete', { title: item.title })">
              <Button :label="t('screenTools.media.cancel')" text @click="confirming = undefined" />
              <Button :label="t('screenTools.media.confirmDelete')" severity="danger" @click="remove(item)" />
            </span>
            <Button
              v-else
              icon="pi pi-trash"
              text
              severity="danger"
              :aria-label="t('screenTools.media.delete', { title: item.title })"
              @click="confirming = item.id"
            />
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { MEDIA_ACCEPT, importMedia, measureUnmeasured, saveMedia } from './media'
import { formatLength } from './music'

const emit = defineEmits<{ error: [unknown]; imported: [number]; loaded: [Schemas['MediaItem'][]] }>()
const { t } = useI18n()
const input = ref<HTMLInputElement>()
const items = shallowRef<Schemas['MediaItem'][]>()
const failed = ref('')
const importErrors = ref<string[]>([])
const busy = ref(false)
// The item whose delete waits for a second press.
const confirming = ref<string>()

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))

async function load() {
  try {
    items.value = await api(client.GET('/api/admin/screen-tools/media'))
    failed.value = ''
    emit('loaded', items.value)
  } catch (e) {
    failed.value = message(e)
  }
}

/** Loads the list, then reads the lengths still missing and shows them. */
async function refresh() {
  await load()
  if (items.value && (await measureUnmeasured(items.value))) await load()
}

async function choose() {
  const files = [...(input.value?.files ?? [])]
  busy.value = true
  importErrors.value = []
  let imported = 0
  for (const file of files) {
    try {
      await importMedia(file)
      imported++
    } catch (e) {
      importErrors.value.push(t('screenTools.media.importFailed', { file: file.name, reason: message(e) }))
    }
  }
  busy.value = false
  if (input.value) input.value.value = ''
  if (imported) emit('imported', imported)
  await refresh()
}

async function setSeconds(item: Schemas['MediaItem'], seconds: number | null) {
  if (!seconds) return
  try {
    await saveMedia(item, { duration_ms: Math.round(seconds * 1000) })
    await load()
  } catch (e) {
    emit('error', e)
  }
}

async function remove(item: Schemas['MediaItem']) {
  confirming.value = undefined
  try {
    await api(client.DELETE('/api/admin/screen-tools/media/{id}', { params: { path: { id: item.id } } }))
    await load()
    input.value?.focus()
  } catch (e) {
    emit('error', e)
  }
}

void refresh()
</script>

<style scoped>
.table {
  width: 100%;
  max-width: 52rem;
  border-collapse: collapse;
}

.table th,
.table td {
  padding: 0.5rem;
  text-align: left;
  border-bottom: 1px solid var(--p-content-border-color);
}

.muted {
  margin-inline-start: 0.5rem;
  color: var(--p-text-muted-color);
}

.seconds {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}

.seconds :deep(.p-inputnumber-input) {
  width: 5rem;
}

.end {
  text-align: right;
}

.errors {
  margin: 0;
}
</style>
