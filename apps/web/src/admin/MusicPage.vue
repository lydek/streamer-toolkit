<template>
  <FullPage :title="t('modules.names.bgm')" tile="bgm" overlay="bgm" :status="live.bgm ? playerLine(live.bgm.payload) : undefined">
    <section class="stack" aria-labelledby="music-player">
      <h2 id="music-player">{{ t('music.player.heading') }}</h2>
      <MusicPlayer :event="live.bgm" :playlists="library?.playlists ?? []" @error="report" />
    </section>

    <section class="stack" aria-labelledby="music-rules">
      <h2 id="music-rules">{{ t('music.rules.heading') }}</h2>
      <SceneRules
        v-if="library"
        :rules="library.scene_rules"
        :playlists="library.playlists"
        :obs-scenes="live.obsState?.scenes.map((s) => s.name) ?? []"
        @saved="load"
        @error="report"
      />
    </section>

    <section class="stack" aria-labelledby="music-library">
      <h2 id="music-library">{{ t('music.library') }}</h2>
      <label for="music-file">{{ t('music.file') }}</label>
      <input
        id="music-file"
        ref="input"
        type="file"
        multiple
        accept=".mp3,.wav,.ogg,audio/mpeg,audio/wav,audio/ogg"
        :disabled="busy"
        @change="choose"
      />
      <ul v-if="importErrors.length" role="alert" class="errors">
        <li v-for="e in importErrors" :key="e">{{ e }}</li>
      </ul>

      <p v-if="failed" role="alert">{{ failed }}</p>
      <p v-else-if="!library">{{ t('app.loading') }}</p>
      <p v-else-if="library.tracks.length === 0">{{ t('music.empty') }}</p>
      <ul v-else class="rows">
        <MusicRow
          v-for="track in library.tracks"
          :key="track.id"
          :track="track"
          :playlists="library.playlists"
          @changed="load"
          @deleted="deleted"
          @error="report"
        />
      </ul>
    </section>

    <section class="stack" aria-labelledby="music-playlists">
      <h2 id="music-playlists">{{ t('music.playlists') }}</h2>
      <form class="create" @submit.prevent="create">
        <div class="field">
          <label for="new-playlist">{{ t('music.newPlaylist') }}</label>
          <InputText id="new-playlist" v-model="newName" autocomplete="off" maxlength="100" />
        </div>
        <Button type="submit" :label="t('music.addPlaylist')" icon="pi pi-plus" :disabled="!newName.trim() || creating" />
      </form>
      <ul v-if="library?.playlists.length" class="rows">
        <PlaylistRow
          v-for="p in library.playlists"
          :key="p.id"
          :playlist="p"
          @changed="load"
          @deleted="playlistDeleted"
          @error="report"
        />
      </ul>
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast, useLiveEvents } from './home'
import { importMusic, measureUnmeasured, playerLine } from './music'
import MusicPlayer from './MusicPlayer.vue'
import MusicRow from './MusicRow.vue'
import { keepFocus } from './page'
import PlaylistRow from './PlaylistRow.vue'
import SceneRules from './SceneRules.vue'

const { t } = useI18n()
const toast = useToast()
const input = ref<HTMLInputElement>()
const library = shallowRef<Schemas['MusicLibrary']>()
const failed = ref('')
const importErrors = ref<string[]>([])
const busy = ref(false)
const newName = ref('')
const creating = ref(false)

const live = useLiveEvents()

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))
const report = (e: unknown) => toast.add(errorToast(message(e)))

async function load() {
  try {
    library.value = await api(client.GET('/api/admin/music'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : t('music.loadFailed')
  }
}

/** Loads the library, then measures the tracks without a length and shows them once saved. */
async function refresh() {
  await load()
  if (library.value && (await measureUnmeasured(library.value.tracks))) await load()
}

async function choose() {
  importErrors.value = []
  const files = [...(input.value?.files ?? [])]
  if (!files.length) return
  busy.value = true
  let imported = 0
  // One at a time: each file is up to 50 MB.
  for (const file of files) {
    try {
      await importMusic(file)
      imported++
    } catch (e) {
      importErrors.value.push(t('music.importFailed', { file: file.name, reason: message(e) }))
    }
  }
  busy.value = false
  if (input.value) input.value.value = ''
  if (imported) toast.add(successToast(t('music.imported', { n: imported })))
  await refresh()
}

async function create() {
  creating.value = true
  try {
    await api(client.POST('/api/admin/music/playlists', { body: { name: newName.value } }))
    newName.value = ''
    await load()
  } catch (e) {
    report(e)
  } finally {
    creating.value = false
  }
}

async function deleted(track: Schemas['Track']) {
  toast.add(successToast(t('music.deleted', { title: track.title })))
  await load()
  // The deleted row took the focused button with it.
  await keepFocus(() => input.value)
}

async function playlistDeleted(p: Schemas['MusicPlaylist']) {
  toast.add(successToast(t('music.deleted', { title: p.name })))
  await load()
  await keepFocus(() => document.getElementById('new-playlist') ?? undefined)
}

void refresh()
</script>

<style scoped>
.copy {
  align-self: flex-start;
}

.rows,
.errors {
  list-style: none;
  padding: 0;
  margin: 0;
}

.create {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.5rem 1rem;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
</style>
