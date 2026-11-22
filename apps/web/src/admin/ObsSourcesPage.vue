<template>
  <FullPage :title="t('pages.obsSources.name')">
    <p v-for="d in duplicates" :key="d.key" role="alert" class="note-bar">
      <i class="pi pi-exclamation-triangle" aria-hidden="true"></i>
      {{ t('obsSources.duplicate', { name: rowName(d), n: d.sources.length, effect: t(`obsSources.twice.${d.entry.id}`) }) }}
    </p>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="data && !data.connected">{{ t('obsSources.notConnected') }}</p>
    <p v-else-if="data && !data.local">{{ t('obsSources.notLocal') }}</p>
    <div v-if="data?.connected && data.scenes.length" class="field">
      <label for="obs-scene">{{ t('obsSources.scene') }}</label>
      <Select v-model="scene" input-id="obs-scene" :options="data.scenes" />
    </div>
    <table class="table">
      <thead>
        <tr>
          <th scope="col">{{ t('obsSources.overlay') }}</th>
          <th scope="col">{{ t('obsSources.shows') }}</th>
          <th v-if="scene" scope="col">{{ t('obsSources.inScene', { scene }) }}</th>
          <th v-if="scene" scope="col">{{ t('obsSources.otherScenes') }}</th>
          <th scope="col">{{ t('obsSources.size') }}</th>
          <th scope="col"><span class="sr-only">{{ t('obsSources.actions') }}</span></th>
        </tr>
      </thead>
      <tbody v-for="g in groups" :key="g.id">
        <tr class="group">
          <th :colspan="scene ? 6 : 4" scope="colgroup">{{ t(`nav.groups.${g.id}`) }}</th>
        </tr>
        <tr v-for="r in g.rows" :key="r.key" :class="{ off: !r.entry.path }">
          <th scope="row">
            {{ rowName(r) }}
            <span v-if="r.sources.length > 1" class="chip">{{ t('obsSources.sources', { n: r.sources.length }) }}</span>
            <span v-if="!r.entry.path" class="sr-only">{{ t('home.unbuilt') }}</span>
          </th>
          <td class="muted">{{ t(r.key === 'leaderboard-other' ? 'obsSources.desc.leaderboardOther' : `obsSources.desc.${r.entry.id}`) }}</td>
          <td v-if="scene">
            <ToggleSwitch
              v-if="r.url"
              :model-value="r.scenes.includes(scene)"
              :disabled="!data?.local || !!busy"
              :aria-label="t('obsSources.inSceneFor', { name: rowName(r), scene })"
              @update:model-value="(on: boolean) => setInScene(r, on)"
            />
          </td>
          <td v-if="scene">
            <details v-if="others(r).length" class="others">
              <summary>{{ t('obsSources.moreScenes', { n: others(r).length }) }}</summary>
              <ul>
                <li v-for="s in others(r)" :key="s">
                  {{ s }}<span v-if="s === data?.live_scene" class="muted">{{ t('obsSources.live') }}</span>
                </li>
              </ul>
            </details>
            <span v-else class="muted">—</span>
          </td>
          <td>{{ r.entry.size === 'audio' ? t('obsSources.audioOnly') : r.entry.size }}</td>
          <td class="end">
            <Button
              v-if="r.url"
              :label="t('obsSources.copy')"
              icon="pi pi-copy"
              text
              :aria-label="t('obsSources.copyFor', { name: rowName(r) })"
              @click="copy(r.url)"
            />
          </td>
        </tr>
      </tbody>
    </table>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { computed, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { useModules } from './features'
import FullPage from './FullPage.vue'
import { errorToast, successToast, useLiveEvents } from './home'
import { type OverlayRow, overlayName, overlayRows } from './obsSources'
import { GROUPS } from './tabs'

const { t } = useI18n()
const toast = useToast()
const { modules } = useModules()

const data = shallowRef<Schemas['ObsSources']>()
const failed = ref('')
// The scene the table looks at: the one on air until another is picked.
const scene = ref<string | null>(null)

async function load() {
  try {
    data.value = await api(client.GET('/api/admin/obs/sources'))
    failed.value = ''
    if (!scene.value || !data.value.scenes.includes(scene.value)) scene.value = data.value.live_scene ?? null
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}

const off = computed(() => new Set((modules.value ?? []).filter((m) => !m.enabled).map((m) => m.id)))
const rows = computed(() => overlayRows(data.value?.sources ?? [], off.value))
const groups = computed(() =>
  GROUPS.map((g) => ({ id: g.id, rows: rows.value.filter((r) => r.entry.group === g.id) })).filter((g) => g.rows.length),
)
const duplicates = computed(() => rows.value.filter((r) => r.sources.length > 1))
const others = (r: OverlayRow) => r.scenes.filter((s) => s !== scene.value)

const rowName = (r: OverlayRow) => overlayName(r, t)

// The row whose switch is saving; the others wait so two changes never cross.
const busy = ref('')
const sizeOf = (size: string): [number, number] => {
  const [width, height] = size === 'audio' ? [100, 100] : size.split(' × ').map(Number)
  return [width, height]
}

/** Puts the overlay in the picked scene or takes it out; a failure leaves the switch as OBS has it. */
async function setInScene(r: OverlayRow, on: boolean) {
  if (!scene.value || !r.url) return
  busy.value = r.key
  try {
    if (on) {
      const [width, height] = sizeOf(r.entry.size)
      const name = t('obsSources.sourceName', { name: rowName(r), app: t('app.name') })
      await api(client.POST('/api/admin/obs/sources', { body: { scene: scene.value, overlay: r.url, name, width, height } }))
    } else {
      await api(client.DELETE('/api/admin/obs/sources', { params: { query: { scene: scene.value, overlay: r.url } } }))
    }
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = ''
    await load()
  }
}

async function copy(path: string) {
  try {
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}${path}`)
    toast.add(successToast(t('chat.copied')))
  } catch {
    toast.add(errorToast(t('chat.copyFailed')))
  }
}

void load()
// Scenes and items come and go with OBS; read the sources again when its state changes.
const live = useLiveEvents()
watch(
  () => live.value.obsState,
  () => void load(),
)
</script>

<style scoped>
.note-bar {
  display: flex;
  gap: 0.5rem;
  align-items: baseline;
  padding: 0.5rem 0.75rem;
  border: 1px solid var(--p-yellow-700);
  border-radius: 6px;
  color: var(--p-yellow-200);
}

.field {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.25rem;
  margin-bottom: 1rem;
}

.table {
  max-width: 72rem;
}

.group th {
  padding-top: 1rem;
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
  font-weight: 400;
}

tbody th[scope='row'] {
  font-weight: 600;
}

.off {
  color: var(--p-text-muted-color);
}

.off th[scope='row'] {
  font-weight: 400;
}

.muted {
  color: var(--p-text-muted-color);
}

.chip {
  margin-inline-start: 0.5rem;
  padding: 0 0.375rem;
  border: 1px solid var(--p-yellow-700);
  border-radius: 999px;
  color: var(--p-yellow-200);
  font-size: 0.75rem;
  font-weight: 400;
}

.pi-check {
  margin-inline-end: 0.25rem;
  color: var(--p-green-400);
}

.others summary {
  cursor: pointer;
  color: var(--p-primary-color);
}

.others ul {
  margin: 0.25rem 0 0;
  padding-inline-start: 1rem;
}

.others .muted {
  margin-inline-start: 0.5rem;
}

.end {
  text-align: end;
}
</style>
