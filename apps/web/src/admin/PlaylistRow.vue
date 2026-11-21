<template>
  <li class="playlist">
    <div class="field">
      <label :for="`playlist-name-${playlist.id}`">{{ t('music.playlistName') }}</label>
      <InputText
        :id="`playlist-name-${playlist.id}`"
        v-model="name.value.value"
        autocomplete="off"
        maxlength="100"
        @keydown.enter="name.commit()"
        @blur="name.commit()"
      />
    </div>
    <span class="count">{{ t('music.count', { n: playlist.tracks.length }) }}</span>
    <Button ref="deleteButton" :label="t('music.delete')" icon="pi pi-trash" severity="danger" text @click="ask" />
    <div v-if="confirming" class="confirm" role="group" :aria-label="t('music.delete')">
      <p>{{ t('music.deletePlaylistNote') }}</p>
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
import { nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'
import { useDrawerSetting } from './drawer'

const props = defineProps<{ playlist: Schemas['MusicPlaylist'] }>()
const emit = defineEmits<{ changed: []; deleted: [Schemas['MusicPlaylist']]; error: [unknown] }>()
const { t } = useI18n()

const nameSaved = ref(props.playlist.name)
const name = useDrawerSetting(nameSaved, async (v) => {
  await api(client.PUT('/api/admin/music/playlists/{id}', { params: { path: { id: props.playlist.id } }, body: { name: v } }))
  // The tracks' playlist choices show the new name.
  emit('changed')
})

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
    await api(client.DELETE('/api/admin/music/playlists/{id}', { params: { path: { id: props.playlist.id } } }))
    emit('deleted', props.playlist)
  } catch (e) {
    emit('error', e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.playlist {
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

.count {
  padding-bottom: 0.5rem;
  color: var(--p-text-muted-color);
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
