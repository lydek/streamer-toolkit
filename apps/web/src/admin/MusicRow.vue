<template>
  <li class="track">
    <Button
      :icon="playing ? 'pi pi-stop' : 'pi pi-play'"
      :aria-label="t(playing ? 'music.stop' : 'music.play', { title: title.value.value })"
      :aria-pressed="playing"
      :disabled="track.missing"
      text
      rounded
      @click="toggle"
    />
    <div class="field">
      <label :for="`music-title-${track.id}`">{{ t('music.title') }}</label>
      <InputText
        :id="`music-title-${track.id}`"
        v-model="title.value.value"
        autocomplete="off"
        maxlength="100"
        @keydown.enter="title.commit()"
        @blur="title.commit()"
      />
    </div>
    <span class="length">
      <span v-if="track.missing" class="missing">{{ t('music.missing') }}</span>
      <template v-else>{{ track.duration_ms === null ? '' : formatLength(track.duration_ms) }}</template>
    </span>
    <div class="field playlists">
      <label :id="`music-playlists-${track.id}`">{{ t('music.playlists') }}</label>
      <MultiSelect
        :model-value="inPlaylists"
        :options="playlists"
        option-label="name"
        option-value="id"
        display="chip"
        :placeholder="t('music.noPlaylist')"
        :aria-labelledby="`music-playlists-${track.id}`"
        :disabled="playlists.length === 0"
        @update:model-value="choose"
      />
    </div>
    <Button ref="deleteButton" :label="t('music.delete')" icon="pi pi-trash" severity="danger" text @click="ask" />
    <div v-if="confirming" class="confirm" role="group" :aria-label="t('music.delete')">
      <p>{{ t('music.deleteNote') }}</p>
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

const playing = ref(false)
let audio: HTMLAudioElement | undefined
function toggle() {
  audio?.pause()
  if (playing.value) {
    playing.value = false
    return
  }
  // The read-only music route; the admin window is on the Host's origin.
  audio = new Audio(`/music/${props.track.id}`)
  audio.onended = audio.onerror = () => (playing.value = false)
  playing.value = true
  audio.play()?.catch(() => (playing.value = false))
}
onBeforeUnmount(() => audio?.pause())

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
.track {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.5rem 1rem;
  padding: 0.5rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.field :deep(.p-inputtext) {
  max-width: 100%;
}

.playlists {
  flex: 1 1 12rem;
}

.length {
  min-width: 4rem;
  padding-bottom: 0.5rem;
  font-variant-numeric: tabular-nums;
}

.missing {
  color: var(--p-red-500);
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.confirm {
  flex-basis: 100%;
}

.confirm p {
  margin: 0 0 0.5rem;
}
</style>
