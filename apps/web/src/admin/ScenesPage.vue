<template>
  <FullPage :title="t('modules.names.scene')" tile="scene" :status="state?.scene ? t('scene.current', { scene: state.scene }) : undefined">
    <p v-if="live.status === 'loading'">{{ t('app.loading') }}</p>
    <!-- OBS down: the page's notice (FullPage) says so; nothing more here. -->
    <template v-else-if="!obsReady(state) && servicesDown.obs" />
    <p v-else-if="!obsReady(state)" role="status">
      {{ t('scene.notConnected') }}
      <RouterLink to="/settings/connections#obs">{{ t('scene.connect') }}</RouterLink>
    </p>
    <!-- Two panes that scroll on their own (wide and tall enough): scenes on the left, the edited scene's items on the
         right, so a long list on either side never pushes the other out of reach. -->
    <div v-else class="scene-panes">
      <section class="stack pane" aria-labelledby="scene-list">
        <h2 id="scene-list">{{ t('scene.scenes') }}</h2>
        <div class="studio">
          <span class="switch">
            <ToggleSwitch
              input-id="studio-mode"
              :model-value="state.studioMode"
              :disabled="studioBusy"
              @update:model-value="(v: boolean) => studio(() => setStudioMode(v))"
            />
            <label for="studio-mode">{{ t('scene.studio') }}</label>
          </span>
          <template v-if="state.studioMode">
            <Select
              v-model="transition"
              :options="state.transitions ?? []"
              :aria-label="t('scene.transitionKind')"
              :disabled="studioBusy"
            />
            <Button
              :label="t('scene.transition')"
              icon="pi pi-step-forward"
              :disabled="studioBusy || !state.preview"
              @click="studio(() => transitionNow(transition ?? undefined))"
            />
          </template>
        </div>
        <p v-if="state.studioMode" class="muted">{{ t('scene.studioNote') }}</p>
        <SceneList :state="state" @error="report" />
      </section>

      <!-- In studio mode everything here edits the preview scene; otherwise the scene on air. -->
      <section v-if="current" class="stack pane" aria-labelledby="scene-items">
        <div class="items-head">
          <h2 id="scene-items">{{ t('scene.items', { scene: current.name }) }}</h2>
          <span class="stag" :class="editingPreview ? 'preview' : 'live'">{{ t(editingPreview ? 'scene.preview' : 'scene.onAir') }}</span>
          <div class="add">
            <Button
              :label="t('scene.addOverlay')"
              icon="pi pi-plus"
              size="small"
              text
              :disabled="!obsSources?.local || !addable.length || adding"
              aria-haspopup="true"
              aria-controls="add-overlay"
              @click="addMenu?.toggle($event)"
            />
            <Menu id="add-overlay" ref="addMenu" :model="addable" popup />
            <span v-if="obsSources && !obsSources.local" class="muted">{{ t('obsSources.notLocal') }}</span>
          </div>
        </div>
        <div class="items-scroll">
        <p v-if="!current.items.length">{{ t('scene.noItems') }}</p>
        <table v-else class="table">
          <caption>{{ t('scene.order') }}</caption>
          <thead>
            <tr>
              <th scope="col">{{ t('scene.item') }}</th>
              <th scope="col">{{ t('scene.kind') }}</th>
              <th scope="col">{{ t('scene.shown') }}</th>
              <th scope="col">{{ t('scene.locked') }}</th>
              <th scope="col">{{ t('scene.move') }}</th>
              <th scope="col">X</th>
              <th scope="col">Y</th>
              <th scope="col">{{ t('scene.align') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(item, i) in current.items" :key="`${item.id}:${resets}`">
              <th scope="row">
                {{ item.source }}
                <RouterLink v-if="duplicates.has(item.source)" to="/obs-sources" class="dup">{{ t('scene.duplicate') }}</RouterLink>
              </th>
              <td>{{ kindText(item.kind) }}</td>
              <td>
                <ToggleSwitch
                  :model-value="item.enabled"
                  :disabled="busy.has(item.id)"
                  :aria-label="t('scene.showItem', { item: item.source })"
                  @update:model-value="(v: boolean) => change(item, { enabled: v })"
                />
              </td>
              <td>
                <ToggleSwitch
                  :model-value="item.locked"
                  :disabled="busy.has(item.id)"
                  :aria-label="t('scene.lockItem', { item: item.source })"
                  @update:model-value="(v: boolean) => change(item, { locked: v })"
                />
              </td>
              <td class="move">
                <Button
                  icon="pi pi-arrow-up"
                  text
                  rounded
                  :disabled="i === 0 || busy.has(item.id)"
                  :aria-label="t('scene.forward', { item: item.source })"
                  @click="change(item, { position: i - 1 })"
                />
                <Button
                  icon="pi pi-arrow-down"
                  text
                  rounded
                  :disabled="i === current.items.length - 1 || busy.has(item.id)"
                  :aria-label="t('scene.backward', { item: item.source })"
                  @click="change(item, { position: i + 1 })"
                />
              </td>
              <template v-if="placements.get(item.id)">
                <td>
                  <InputNumber
                    :model-value="placements.get(item.id)!.x"
                    :max-fraction-digits="1"
                    :disabled="busy.has(item.id)"
                    :aria-label="t('scene.x', { item: item.source })"
                    class="coord"
                    @update:model-value="(x: number | null) => x !== null && place(item, { x })"
                  />
                </td>
                <td>
                  <InputNumber
                    :model-value="placements.get(item.id)!.y"
                    :max-fraction-digits="1"
                    :disabled="busy.has(item.id)"
                    :aria-label="t('scene.y', { item: item.source })"
                    class="coord"
                    @update:model-value="(y: number | null) => y !== null && place(item, { y })"
                  />
                </td>
                <td>
                  <Select
                    :model-value="placements.get(item.id)!.align"
                    :options="ALIGNS"
                    :option-label="(a: Align) => t(`scene.aligns.${a}`)"
                    :disabled="busy.has(item.id)"
                    :aria-label="t('scene.alignOf', { item: item.source })"
                    @update:model-value="(align: Align) => place(item, { align })"
                  />
                  <span v-if="outside(placements.get(item.id)!)" class="warn">{{ t('scene.outside') }}</span>
                </td>
              </template>
              <td v-else colspan="3"></td>
            </tr>
          </tbody>
        </table>
        </div>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import Menu from 'primevue/menu'
import type { MenuItem } from 'primevue/menuitem'
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, servicesDown, useLiveEvents } from './home'
import { addOverlay, overlayName, overlayRows, useObsSources } from './obsSources'
import SceneList from './SceneList.vue'
import { type SceneItem, kindKey, obsReady, setStudioMode, transitionNow, updateItem } from './scenes'

const { t } = useI18n()
const toast = useToast()
const live = useLiveEvents()
const state = computed(() => live.value.obsState)
// The scene this page edits: the preview in studio mode, else the one on air.
const editingPreview = computed(() => !!state.value?.studioMode && !!state.value.preview)
const target = computed(() => (editingPreview.value ? state.value?.preview : state.value?.scene) ?? undefined)
const current = computed(() => state.value?.scenes.find((s) => s.name === target.value))
// Items with a command in flight; their switches wait for it.
const busy = ref(new Set<number>())
// A switch shows the change it asked for until OBS reports it; a refused one is rebuilt from OBS's state.
const resets = ref(0)

const kindText = (kind: string) => {
  const key = kindKey(kind)
  return key ? t(key) : kind || '—'
}
const report = (e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))

// Sources showing an overlay that has another source too; read again as OBS's scenes and items change.
const obsSources = useObsSources()
watch(state, () => void useObsSources(true))
const duplicates = computed(
  () =>
    new Set(
      overlayRows(obsSources.value?.sources ?? [])
        .filter((r) => r.sources.length > 1)
        .flatMap((r) => r.sources.map((s) => s.name)),
    ),
)

// Studio mode: the transition picked here, starting from the one in use in OBS.
const transition = ref<string | null>(null)
watch(
  () => state.value?.transition,
  (now) => (transition.value = now ?? null),
  { immediate: true },
)
const studioBusy = ref(false)
async function studio(command: () => Promise<unknown>) {
  studioBusy.value = true
  try {
    await command()
  } catch (e) {
    report(e)
  } finally {
    studioBusy.value = false
  }
}

// 加入 Overlay: the built-in overlays not in this scene yet, with their suggested size.
const addMenu = ref<InstanceType<typeof Menu>>()
const adding = ref(false)
const addable = computed<MenuItem[]>(() => {
  const scene = target.value
  if (!scene) return []
  return overlayRows(obsSources.value?.sources ?? [])
    .filter((r) => r.url && !r.scenes.includes(scene))
    .map((r) => ({
      label: `${overlayName(r, t)}　${r.entry.size === 'audio' ? t('obsSources.audioOnly') : r.entry.size}`,
      command: async () => {
        adding.value = true
        try {
          await addOverlay(scene, r, t)
        } catch (e) {
          report(e)
        } finally {
          adding.value = false
          void useObsSources(true)
        }
      },
    }))
})

type Align = Schemas['Align']
const ALIGNS: Align[] = ['top_left', 'top', 'top_right', 'left', 'center', 'right', 'bottom_left', 'bottom', 'bottom_right']
// The canvas and each item's position in the edited scene, read again as OBS's scenes and items change.
const canvas = ref<Schemas['Canvas']>()
const placements = ref(new Map<number, Schemas['Placement']>())
// Each read's number; only the latest one's answer shows, so an older scene's never replaces the edited one.
let placementRead = 0
watch(
  [state, target],
  async ([, scene]) => {
    if (!scene) return
    const mine = ++placementRead
    try {
      const got = await api(client.GET('/api/admin/obs/placements', { params: { query: { scene } } }))
      if (mine !== placementRead) return
      canvas.value = got.canvas
      placements.value = new Map(got.items.map(({ item, ...p }) => [item, p]))
    } catch {
      if (mine === placementRead) placements.value = new Map()
    }
  },
  { immediate: true },
)
const outside = (p: Schemas['Placement']) =>
  !!canvas.value && (p.x < 0 || p.y < 0 || p.x > canvas.value.width || p.y > canvas.value.height)

async function place(item: SceneItem, what: Partial<Schemas['Placement']>) {
  const now = placements.value.get(item.id)
  if (!now) return
  const next = { ...now, ...what }
  // OBS sends no event when an item moves, so the next change starts from this one, not the last read.
  if (await change(item, { placement: next })) placements.value = new Map(placements.value).set(item.id, next)
}

async function change(
  item: SceneItem,
  what: { enabled?: boolean; locked?: boolean; position?: number; placement?: Schemas['Placement'] },
) {
  const scene = target.value
  if (!scene) return false
  busy.value = new Set(busy.value).add(item.id)
  try {
    await updateItem(scene, item.id, what)
    return true
  } catch (e) {
    resets.value++
    report(e)
    return false
  } finally {
    const next = new Set(busy.value)
    next.delete(item.id)
    busy.value = next
  }
}
</script>

<style scoped>
/* Wide and tall enough: the page fits the window and each pane scrolls; the table head stays. */
@media (min-height: 560px) {
  @container view (min-width: 64.0625rem) {
    .scene-panes {
      display: grid;
      grid-template-columns: minmax(16rem, 22rem) minmax(0, 1fr);
      gap: 1rem;
      height: calc(100vh - var(--appbar-h, 0px) - var(--page-top-h, 0px) - 3rem);
      min-height: 24rem;
    }

    .pane {
      display: flex;
      flex-direction: column;
      min-height: 0;
      overflow-y: auto;
    }

    .items-scroll {
      min-height: 0;
      overflow: auto;
    }

    .items-scroll thead th {
      position: sticky;
      top: 0;
      z-index: 1;
      background: var(--p-surface-950);
    }
  }
}

/* One column: the scene list keeps to about three rows, so the items are near. */
@container view (max-width: 64rem) {
  .pane:first-child :deep(.scenes) {
    max-height: 11.5rem;
    overflow-y: auto;
  }
}

.items-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.25rem 0.75rem;
}

.items-head h2 {
  margin: 0;
}

.stag {
  padding: 0 0.5rem;
  border-radius: 999px;
  font-size: 0.75rem;
}

.stag.live {
  background: color-mix(in srgb, var(--p-red-400) 20%, transparent);
  color: var(--p-red-300);
}

.stag.preview {
  background: color-mix(in srgb, var(--p-green-400) 20%, transparent);
  color: var(--p-green-300);
}

.table {
  max-width: 40rem;
}

caption {
  text-align: start;
  color: var(--p-text-muted-color);
  padding-bottom: 0.25rem;
}

tbody th {
  font-weight: normal;
}

.dup {
  margin-inline-start: 0.5rem;
  padding: 0 0.375rem;
  border: 1px solid var(--p-yellow-700);
  border-radius: 999px;
  color: var(--p-yellow-200);
  font-size: 0.75rem;
  font-weight: 400;
  text-decoration: none;
}

.move {
  white-space: nowrap;
}

.coord :deep(.p-inputnumber-input) {
  width: 5.5rem;
}

.warn {
  display: block;
  color: var(--p-yellow-300);
  font-size: 0.8125rem;
}

.add {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1rem;
  align-items: center;
  margin-left: auto;
}

.muted {
  color: var(--p-text-muted-color);
}

.studio {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1rem;
  align-items: center;
}

.switch {
  display: inline-flex;
  gap: 0.5rem;
  align-items: center;
}
</style>
