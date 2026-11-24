<template>
  <tbody class="playlist">
    <tr>
      <th scope="row">
        <b>{{ name.value.value }}</b>
      </th>
      <td class="count">{{ t('music.count', { n: playlist.tracks.length }) }}</td>
      <td>
        <span v-if="playlist.used_by.length" class="chips">
          <Tag v-for="(u, i) in playlist.used_by" :key="i" :value="u" severity="secondary" />
        </span>
        <span v-else class="muted">{{ t('music.notUsed') }}</span>
      </td>
      <td class="end">
        <span class="row-actions">
          <Button
            icon="pi pi-pencil"
            text
            size="small"
            :aria-label="t('music.rules.editFor', { name: name.value.value })"
            :aria-expanded="editing"
            :aria-controls="`playlist-detail-${playlist.id}`"
            @click="toggleEdit"
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
        </span>
      </td>
    </tr>

    <tr v-show="editing || confirming" :id="`playlist-detail-${playlist.id}`" class="detail-row">
      <td colspan="4">
        <div class="detail">
          <div v-if="editing" class="field">
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
          <div v-if="confirming" class="confirm" role="group" :aria-label="t('music.delete')">
            <p>{{ t('music.deletePlaylistNote') }}</p>
            <template v-if="playlist.used_by.length">
              <p>{{ t('music.playlistUsedBy') }}</p>
              <ul>
                <li v-for="(u, i) in playlist.used_by" :key="i">{{ u }}</li>
              </ul>
            </template>
            <div class="buttons">
              <Button ref="confirmButton" :label="t('music.confirmDelete')" severity="danger" :disabled="busy" @click="remove" />
              <Button :label="t('music.cancel')" text @click="cancel" />
            </div>
          </div>
        </div>
      </td>
    </tr>
  </tbody>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Tag from 'primevue/tag'
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

const editing = ref(false)
async function toggleEdit() {
  editing.value = !editing.value
  if (editing.value) {
    await nextTick()
    document.getElementById(`playlist-name-${props.playlist.id}`)?.focus()
  }
}
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
.count {
  color: var(--p-text-muted-color);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.chips {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 0.25rem;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.end {
  text-align: right;
  white-space: nowrap;
}

.row-actions {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
}

.detail {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0.5rem 0;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  max-width: 20rem;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.confirm p,
.confirm ul {
  margin: 0 0 0.5rem;
}
</style>
