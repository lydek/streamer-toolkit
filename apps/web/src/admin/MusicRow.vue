<template>
  <li class="track sound-card">
    <div class="sound-row">
      <Button
        :icon="playing ? 'pi pi-stop' : 'pi pi-play'"
        :aria-label="t(playing ? 'music.stop' : 'music.play', { title: title.value.value })"
        :aria-pressed="playing"
        :disabled="track.missing"
        severity="secondary"
        rounded
        @click="togglePreview(url)"
      />
      <InputText
        :id="`music-title-${track.id}`"
        v-model="title.value.value"
        class="title-input"
        autocomplete="off"
        maxlength="100"
        :aria-label="t('music.title')"
        @keydown.enter="title.commit()"
        @blur="title.commit()"
      />
      <span class="length">
        <span v-if="track.missing" class="missing">{{ t('music.missing') }}</span>
        <template v-else>{{ track.duration_ms === null ? '' : formatLength(track.duration_ms) }}</template>
      </span>
      <MultiSelect
        :model-value="inPlaylists"
        class="playlists"
        :options="playlists"
        option-label="name"
        option-value="id"
        display="chip"
        :placeholder="t('music.noPlaylist')"
        :aria-label="t('music.playlists')"
        :disabled="playlists.length === 0"
        @update:model-value="choose"
      />
      <Button
        ref="deleteButton"
        :label="t('music.delete')"
        icon="pi pi-trash"
        severity="danger"
        text
        size="small"
        @click="ask"
      />
    </div>
    <div v-if="confirming" class="confirm" role="group" :aria-label="t('music.delete')">
      <p>{{ t('music.deleteNote') }}</p>
      <template v-if="track.used_by.length">
        <p>{{ t('music.trackUsedBy') }}</p>
        <ul>
          <li v-for="(u, i) in track.used_by" :key="i">{{ u }}</li>
        </ul>
      </template>
      <div class="buttons">
        <Button ref="confirmButton" :label="t('music.confirmDelete')" severity="danger" :disabled="busy" @click="remove" />
        <Button :label="t('music.cancel')" text @click="cancel" />
      </div>
    </div>
  </li>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import MultiSelect from 'primevue/multiselect'
import { computed, nextTick, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'
import { useDrawerSetting } from './drawer'
import { previewUrl, stopPreview, togglePreview } from './preview'
import { formatLength } from './music'

const props = defineProps<{ track: Schemas['Track']; playlists: Schemas['MusicPlaylist'][] }>()
const emit = defineEmits<{ changed: []; deleted: [Schemas['Track']]; error: [unknown] }>()
const { t } = useI18n()

const titleSaved = ref(props.track.title)
const title = useDrawerSetting(titleSaved, (v) =>
  api(client.PUT('/api/admin/music/{id}', { params: { path: { id: props.track.id } }, body: { title: v } })),
)

const inPlaylists = computed(() => props.playlists.filter((p) => p.tracks.includes(props.track.id)).map((p) => p.id))
async function choose(ids: string[]) {
  try {
    await api(client.PUT('/api/admin/music/{id}/playlists', { params: { path: { id: props.track.id } }, body: { playlists: ids } }))
  } catch (e) {
    emit('error', e)
  }
  // Either way the list shows what the Host has.
  emit('changed')
}

// The read-only music route; the admin window is on the Host's origin. One preview plays at a time.
const url = `/music/${props.track.id}`
const playing = computed(() => previewUrl.value === url)
onBeforeUnmount(() => stopPreview(url))

const confirming = ref(false)
const busy = ref(false)
const deleteButton = ref<{ $el: HTMLElement }>()
const confirmButton = ref<{ $el: HTMLElement }>()
async function ask() {
  confirming.value = true
  await nextTick()
  confirmButton.value?.$el.focus()
}
async function cancel() {
  confirming.value = false
  await nextTick()
  deleteButton.value?.$el.focus()
}
async function remove() {
  busy.value = true
  try {
    await api(client.DELETE('/api/admin/music/{id}', { params: { path: { id: props.track.id } } }))
    emit('deleted', props.track)
  } catch (e) {
    emit('error', e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.sound-card {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 0.625rem 0.875rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.sound-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.875rem;
}

.title-input {
  flex: 1 1 12rem;
  min-width: 0;
}

.playlists {
  flex: 1 1 12rem;
  min-width: 0;
}

.length {
  min-width: 3.5rem;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
  font-variant-numeric: tabular-nums;
}

.missing {
  color: var(--p-red-400);
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.confirm {
  padding-top: 0.5rem;
  border-top: 1px solid var(--p-surface-800);
}

.confirm p,
.confirm ul {
  margin: 0 0 0.5rem;
}
</style>
