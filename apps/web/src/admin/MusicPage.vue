<template>
  <!-- The background music pages, one part each: 播放 (player and scene rules), 音樂歌單, 音樂庫. -->
  <FullPage tile="bgm" overlay="bgm" :status="live.bgm ? playerLine(live.bgm.payload) : undefined">
    <p v-if="part === 'player' && notInScene" role="alert" class="note-bar">
      <i class="pi pi-exclamation-triangle" aria-hidden="true"></i>
      {{ t('obsSources.problems.bgmNotInScene', { scene: obsSources?.live_scene }) }}
      <Button
        :label="t('music.addToScene')"
        size="small"
        :disabled="!obsSources?.local || adding"
        :title="obsSources?.local ? undefined : t('obsSources.notLocal')"
        @click="addToScene"
      />
    </p>
    <section v-if="part === 'player'" class="section" aria-labelledby="music-player">
      <h2 id="music-player" class="sr-only">{{ t('music.player.heading') }}</h2>
      <MusicPlayer :event="live.bgm" :playlists="library?.playlists ?? []" @error="report" />
    </section>

    <section v-if="part === 'player'" class="section rules-section" aria-labelledby="music-rules">
      <div class="sec-head">
        <h2 id="music-rules">{{ t('music.rules.heading') }}</h2>
        <span class="muted">{{ t('music.rules.hint') }}</span>
      </div>
      <SceneRules
        v-if="library"
        :rules="library.scene_rules"
        :playlists="library.playlists"
        :obs-scenes="live.obsState?.scenes.map((s) => s.name) ?? []"
        @saved="load"
        @error="report"
      />
    </section>

    <section v-if="part === 'library'" class="stack" aria-labelledby="music-library">
      <h2 id="music-library">{{ t('music.library') }}</h2>
      <FilePick ref="pick" input-id="music-file" :label="t('music.file')" icon="pi pi-plus" multiple accept=".mp3,.wav,.ogg,audio/mpeg,audio/wav,audio/ogg" :disabled="busy" @pick="choose" />
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

    <section v-if="part === 'playlists'" class="stack" aria-labelledby="music-playlists">
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
import { computed, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import FilePick from './FilePick.vue'
import { errorToast, successToast, useLiveEvents } from './home'
import { importMusic, measureUnmeasured, playerLine } from './music'
import MusicPlayer from './MusicPlayer.vue'
import MusicRow from './MusicRow.vue'
import { addOverlay, obsProblems, overlayRows, useObsSources } from './obsSources'
import { keepFocus } from './page'
import PlaylistRow from './PlaylistRow.vue'
import SceneRules from './SceneRules.vue'

withDefaults(defineProps<{ part?: 'player' | 'playlists' | 'library' }>(), { part: 'player' })
const { t } = useI18n()
const toast = useToast()
const pick = ref<InstanceType<typeof FilePick>>()
const library = shallowRef<Schemas['MusicLibrary']>()
const failed = ref('')
const importErrors = ref<string[]>([])
const busy = ref(false)
const newName = ref('')
const creating = ref(false)

const live = useLiveEvents()

// The scene on air has a playlist but the music overlay is not in it: say so, and put it there. Read again as
// OBS's scenes and items change, so the warning follows the scene on air.
const obsSources = useObsSources()
watch(
  // Scenes, items and the scene on air: not a fader or mute moving.
  () => JSON.stringify([live.value.obsState?.scene, live.value.obsState?.scenes]),
  () => void useObsSources(true),
)
const notInScene = computed(() =>
  obsProblems(obsSources.value, library.value?.scene_rules ?? [], new Set(), t).some((p) => p.id === 'bgm:scene'),
)
const adding = ref(false)
async function addToScene() {
  const scene = obsSources.value?.live_scene
  const row = overlayRows(obsSources.value?.sources ?? []).find((r) => r.key === 'bgm')
  if (!scene || !row) return
  adding.value = true
  try {
    await addOverlay(scene, row, t)
  } catch (e) {
    report(e)
  } finally {
    adding.value = false
    useObsSources(true)
  }
}

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

async function choose(files: File[]) {
  importErrors.value = []
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
  await keepFocus(() => pick.value?.element())
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

.note-bar {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  align-items: center;
  padding: 0.5rem 0.75rem;
  border: 1px solid var(--p-yellow-700);
  border-radius: 6px;
  color: var(--p-yellow-200);
  margin-bottom: 0.75rem;
}

.section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.rules-section {
  margin-top: 1.25rem;
}

.sec-head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 0.5rem 0.75rem;
}

.sec-head h2 {
  margin: 0;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
