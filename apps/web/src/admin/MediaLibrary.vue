<template>
  <div class="media-lib">
    <ul v-if="importErrors.length" role="alert" class="errors">
      <li v-for="e in importErrors" :key="e">{{ e }}</li>
    </ul>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!items">{{ t('app.loading') }}</p>

    <div v-else class="tbl-wrap media-wrap">
      <table v-if="items.length" class="table">
        <thead>
          <tr>
            <th scope="col">{{ t('screenTools.media.title') }}</th>
            <th scope="col">{{ t('screenTools.media.length') }}</th>
            <th scope="col">{{ t('screenTools.media.usedIn') }}</th>
            <th scope="col"><span class="sr-only">{{ t('screenTools.media.actions') }}</span></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="item in items" :key="item.id" :data-media="item.id">
            <th scope="row">
              <span class="file-cell">
                <span>{{ item.title }}</span>
                <Tag :value="item.format.toUpperCase()" severity="secondary" />
                <span v-if="item.missing" class="missing">{{ t('screenTools.media.missing') }}</span>
              </span>
            </th>
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
              <span v-else class="len">{{ item.duration_ms == null ? t('screenTools.media.reading') : formatLength(item.duration_ms) }}</span>
            </td>
            <td>
              <span v-if="usedBy?.get(item.id)?.length" class="chips">
                <Tag v-for="r in usedBy.get(item.id)" :key="r" :value="r" severity="secondary" />
              </span>
              <span v-else class="muted">—</span>
            </td>
            <td class="end">
              <span v-if="confirming === item.id" role="group" :aria-label="t('screenTools.media.delete', { title: item.title })">
                <Button :label="t('screenTools.media.cancel')" icon="pi pi-times" text @click="cancelDelete(item.id)" />
                <Button :id="`media-confirm-${item.id}`" :label="t('screenTools.media.confirmDelete')" icon="pi pi-trash" severity="danger" @click="remove(item)" />
              </span>
              <Button
                v-else
                :id="`media-delete-${item.id}`"
                icon="pi pi-trash"
                text
                severity="danger"
                :aria-label="t('screenTools.media.delete', { title: item.title })"
                @click="askDelete(item.id)"
              />
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty-cell">{{ t('screenTools.media.empty') }}</p>
      <div class="tbl-foot">
        <span class="muted">{{ t('screenTools.media.fileHint') }}</span>
        <FilePick
          ref="pick"
          input-id="media-file"
          :label="t('screenTools.media.file')"
          icon="pi pi-upload"
          multiple
          :accept="MEDIA_ACCEPT"
          :disabled="busy"
          @pick="choose"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import Tag from 'primevue/tag'
import { nextTick, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import FilePick from './FilePick.vue'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { MEDIA_ACCEPT, importMedia, measureUnmeasured, saveMedia } from './media'
import { formatLength } from './music'

/** `usedBy`: the rewards each file is used by, by item ID. */
defineProps<{ usedBy?: Map<string, string[]> }>()
const emit = defineEmits<{ error: [unknown]; imported: [number]; loaded: [Schemas['MediaItem'][]] }>()
const { t } = useI18n()
const pick = ref<InstanceType<typeof FilePick>>()
const items = shallowRef<Schemas['MediaItem'][]>()
const failed = ref('')
const importErrors = ref<string[]>([])
const busy = ref(false)
// The item whose delete waits for a second press.
const confirming = ref<string>()
async function askDelete(id: string) {
  confirming.value = id
  await nextTick()
  document.getElementById(`media-confirm-${id}`)?.focus()
}
async function cancelDelete(id: string) {
  confirming.value = undefined
  await nextTick()
  document.getElementById(`media-delete-${id}`)?.focus()
}

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

async function choose(files: File[]) {
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
    pick.value?.element()?.focus()
  } catch (e) {
    emit('error', e)
  }
}

void refresh()
</script>

<style scoped>
.media-lib {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.media-wrap {
  max-width: 56rem;
  margin-top: 0;
}

.file-cell {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

.chips {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 0.25rem;
}

.len {
  font-variant-numeric: tabular-nums;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.missing {
  color: var(--p-red-400);
  font-size: 0.8125rem;
}

.empty-cell {
  margin: 0;
  padding: 1rem 0.75rem;
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
  white-space: nowrap;
}

.errors {
  margin: 0;
}
</style>
