<template>
  <FullPage :title="t('modules.names.scene')" tile="scene" :status="state?.scene ? t('scene.current', { scene: state.scene }) : undefined">
    <p v-if="live.status === 'loading'">{{ t('app.loading') }}</p>
    <p v-else-if="!obsReady(state)" role="status">
      {{ t('scene.notConnected') }}
      <RouterLink to="/settings/connections#obs">{{ t('scene.connect') }}</RouterLink>
    </p>
    <template v-else>
      <section class="stack" aria-labelledby="scene-list">
        <h2 id="scene-list">{{ t('scene.scenes') }}</h2>
        <SceneList :state="state" @error="report" />
      </section>

      <section v-if="current" class="stack" aria-labelledby="scene-items">
        <h2 id="scene-items">{{ t('scene.items', { scene: current.name }) }}</h2>
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
            </tr>
          </tbody>
        </table>
      </section>
    </template>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import FullPage from './FullPage.vue'
import { errorToast, useLiveEvents } from './home'
import { overlayRows, useObsSources } from './obsSources'
import SceneList from './SceneList.vue'
import { type SceneItem, kindKey, obsReady, updateItem } from './scenes'

const { t } = useI18n()
const toast = useToast()
const live = useLiveEvents()
const state = computed(() => live.value.obsState)
const current = computed(() => state.value?.scenes.find((s) => s.name === state.value?.scene))
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

async function change(item: SceneItem, what: { enabled?: boolean; locked?: boolean; position?: number }) {
  const scene = state.value?.scene
  if (!scene) return
  busy.value = new Set(busy.value).add(item.id)
  try {
    await updateItem(scene, item.id, what)
  } catch (e) {
    resets.value++
    report(e)
  } finally {
    const next = new Set(busy.value)
    next.delete(item.id)
    busy.value = next
  }
}
</script>

<style scoped>
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
</style>
