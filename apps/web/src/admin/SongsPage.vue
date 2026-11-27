<template>
  <!-- 點歌: the song being sung, the queue and what was sung, plus requesting a song by hand (docs/modules/songs.md). -->
  <FullPage :status="t('songs.summary', { n: board.queued.length })">
    <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
    <form class="add" :aria-label="t('songs.add')" @submit.prevent="add">
      <Select
        v-model="pick"
        :options="songs"
        :option-label="songLabel"
        option-value="id"
        filter
        :filter-placeholder="t('songs.search')"
        :placeholder="t('songs.search')"
        :aria-label="t('songs.song')"
        class="pick"
      />
      <InputText v-model="viewer" :maxlength="VIEWER_MAX" :placeholder="t('songs.for')" :aria-label="t('songs.for')" />
      <Button :label="t('songs.addButton')" icon="pi pi-plus" type="submit" :disabled="!pick || busy" />
    </form>

    <div class="song-board">
      <section class="tbl-wrap now" aria-labelledby="song-now">
        <h3 id="song-now" class="tbl-head">{{ t('songs.now') }}</h3>
        <div v-if="board.singing" class="now-body" data-singing>
          <p class="now-title">
            <b>{{ board.singing.title }}</b> <span class="muted">{{ board.singing.artist }}</span>
          </p>
          <p v-if="board.singing.viewer" class="muted">{{ t('songs.by', { viewer: board.singing.viewer }) }}</p>
        </div>
        <p v-else class="empty-cell">{{ t('songs.nothingNow') }}</p>
        <div class="tbl-foot">
          <Button
            v-if="board.singing"
            :label="t('songs.remove')"
            icon="pi pi-times"
            text
            severity="secondary"
            @click="run(() => api(client.DELETE('/api/admin/song-requests/{id}', { params: { path: { id: board.singing!.id } } })))"
          />
          <span v-else></span>
          <Button :label="t('songs.finish')" icon="pi pi-check" :disabled="!board.singing" @click="run(() => api(client.POST('/api/admin/song-requests/finish')))" />
        </div>
      </section>

      <section class="tbl-wrap" aria-labelledby="song-queue">
        <div class="tbl-top">
          <h3 id="song-queue" class="tbl-head">{{ t('songs.queue') }} <span class="count">{{ board.queued.length }}</span></h3>
        </div>
        <p v-if="!board.queued.length" class="empty-cell">{{ t('songs.emptyQueue') }}</p>
        <ol v-else class="queue">
          <li
            v-for="(r, i) in board.queued"
            :key="r.id"
            :data-request="r.id"
            :class="{ over: over === r.id }"
            draggable="true"
            @dragstart="onDragstart($event, r.id)"
            @dragover="onDragover($event, r.id)"
            @dragleave="over = null"
            @drop="onDrop($event, r.id)"
            @dragend="dragged = over = null"
          >
            <i class="pi pi-bars grip" aria-hidden="true" />
            <span class="n">{{ i + 1 }}</span>
            <span class="t">
              <b>{{ r.title }}</b>
              <small class="muted">{{ detail(r) }}</small>
            </span>
            <span class="acts">
              <Button icon="pi pi-arrow-up" text :disabled="i === 0" :aria-label="`${t('arrange.earlier')}：${r.title}`" @click="move(i, -1)" />
              <Button
                icon="pi pi-arrow-down"
                text
                :disabled="i === board.queued.length - 1"
                :aria-label="`${t('arrange.later')}：${r.title}`"
                @click="move(i, 1)"
              />
              <Button
                v-if="!board.singing"
                icon="pi pi-play"
                text
                :aria-label="t('songs.startLabel', { title: r.title })"
                @click="run(() => api(client.POST('/api/admin/song-requests/{id}/start', { params: { path: { id: r.id } } })))"
              />
              <Button
                icon="pi pi-times"
                text
                :aria-label="t('songs.removeLabel', { title: r.title })"
                @click="run(() => api(client.DELETE('/api/admin/song-requests/{id}', { params: { path: { id: r.id } } })))"
              />
            </span>
          </li>
        </ol>
      </section>

      <section class="tbl-wrap" aria-labelledby="song-sung">
        <h3 id="song-sung" class="tbl-head">{{ t('songs.sung') }} <span class="count">{{ board.sung.length }}</span></h3>
        <p v-if="!board.sung.length" class="empty-cell">{{ t('songs.nothingSung') }}</p>
        <ol v-else class="queue sung">
          <li v-for="r in board.sung" :key="r.id">
            <span class="n">{{ r.finished_at ? clock(r.finished_at) : '' }}</span>
            <span class="t">
              <b>{{ r.title }}</b>
              <small class="muted">{{ detail(r) }}</small>
            </span>
          </li>
        </ol>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast, useLiveEvents } from './home'

type Request = Schemas['RequestView']
type Song = Schemas['SongView']
// The Host's limit, checked again there.
const VIEWER_MAX = 25

const { t } = useI18n()
const toast = useToast()
const board = shallowRef<Schemas['RequestBoard']>({ singing: null, queued: [], sung: [] })
const songs = shallowRef<Song[]>([])
const loadFailed = ref('')
const pick = ref<string>()
const viewer = ref('')
const busy = ref(false)
const dragged = ref<string | null>(null)
const over = ref<string | null>(null)

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))
const songLabel = (s: Song) => (s.artist ? `${s.title} - ${s.artist}` : s.title)
const detail = (r: Request) => [r.artist, r.viewer && `@${r.viewer}`].filter(Boolean).join(' · ')
const clock = (at: string) => new Date(at).toLocaleTimeString('zh-TW', { hour: '2-digit', minute: '2-digit', hour12: false })

async function load() {
  board.value = await api(client.GET('/api/admin/song-requests'))
}

async function loadSongs() {
  songs.value = (await api(client.GET('/api/admin/song-list'))).songs
}

async function run(action: () => Promise<unknown>) {
  try {
    await action()
  } catch (e) {
    toast.add(errorToast(message(e)))
  }
  await load().catch(() => {})
}

async function add() {
  const song = songs.value.find((s) => s.id === pick.value)
  if (!song) return
  busy.value = true
  try {
    await api(client.POST('/api/admin/song-requests', { body: { song_id: song.id, viewer: viewer.value.trim() || null } }))
    toast.add(successToast(t('songs.added', { title: song.title })))
    pick.value = undefined
    viewer.value = ''
  } catch (e) {
    toast.add(errorToast(message(e)))
  } finally {
    busy.value = false
  }
  await load().catch(() => {})
}

/** Saves the queue in this order. */
function reorder(ids: string[]) {
  const byId = new Map(board.value.queued.map((r) => [r.id, r]))
  board.value = { ...board.value, queued: ids.map((id) => byId.get(id)!) }
  void run(() => api(client.PUT('/api/admin/song-requests/order', { body: { ids } })))
}

function move(i: number, by: number) {
  const ids = board.value.queued.map((r) => r.id)
  ;[ids[i], ids[i + by]] = [ids[i + by], ids[i]]
  reorder(ids)
}

function onDragstart(e: DragEvent, id: string) {
  dragged.value = id
  e.dataTransfer?.setData('text/plain', id)
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}

function onDragover(e: DragEvent, id: string) {
  if (!dragged.value || dragged.value === id) return
  e.preventDefault()
  over.value = id
}

function onDrop(e: DragEvent, target: string) {
  const from = dragged.value
  dragged.value = over.value = null
  if (!from || from === target) return
  e.preventDefault()
  const ids = board.value.queued.map((r) => r.id).filter((id) => id !== from)
  ids.splice(ids.indexOf(target), 0, from)
  reorder(ids)
}

// A request from chat or another window changes the queue; a reconnect may have missed some.
useLiveEvents(2000, (msg) => {
  if (msg.type === 'songs.changed' || msg.type === 'snapshot') void load().catch(() => {})
})
load().catch((e: unknown) => (loadFailed.value = message(e)))
loadSongs().catch((e: unknown) => (loadFailed.value = message(e)))
</script>

<style scoped>
.add {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  max-width: 90rem;
  margin-bottom: 1rem;
}

.pick {
  flex: 1 1 18rem;
  min-width: 0;
}

.song-board {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(22rem, 100%), 1fr));
  gap: 1rem;
  align-items: start;
  max-width: 90rem;
}

.song-board .tbl-wrap {
  margin-top: 0;
}

.now-body {
  padding: 0.75rem 1rem;
}

.now-body p {
  margin: 0 0 0.25rem;
}

.now-title b {
  font-size: 1.25rem;
}

.count {
  margin-inline-start: 0.375rem;
  color: var(--p-text-muted-color);
  font-weight: normal;
}

.queue {
  margin: 0;
  padding: 0;
  list-style: none;
}

.queue li {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.375rem 1rem;
  border-top: 1px solid var(--p-content-border-color);
}

.queue li.over {
  outline: 2px dashed var(--p-primary-color);
  outline-offset: -2px;
}

.grip {
  cursor: grab;
  color: var(--p-text-muted-color);
}

.n {
  min-width: 2.5rem;
  color: var(--p-text-muted-color);
  font-variant-numeric: tabular-nums;
}

.t {
  display: grid;
  flex: 1;
  min-width: 0;
}

.acts {
  display: flex;
}

.muted {
  color: var(--p-text-muted-color);
}
</style>
