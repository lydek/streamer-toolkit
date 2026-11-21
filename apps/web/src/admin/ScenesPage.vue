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
        <table v-else>
          <caption>{{ t('scene.order') }}</caption>
          <thead>
            <tr>
              <th scope="col">{{ t('scene.item') }}</th>
              <th scope="col">{{ t('scene.kind') }}</th>
              <th scope="col">{{ t('scene.shown') }}</th>
              <th scope="col">{{ t('scene.locked') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="item in current.items" :key="`${item.id}:${resets}`">
              <th scope="row">{{ item.source }}</th>
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
            </tr>
          </tbody>
        </table>
      </section>
    </template>
  </FullPage>
</template>

<script setup lang="ts">
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import FullPage from './FullPage.vue'
import { errorToast, useLiveEvents } from './home'
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

async function change(item: SceneItem, what: { enabled?: boolean; locked?: boolean }) {
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
table {
  border-collapse: collapse;
  width: 100%;
  max-width: 40rem;
}

caption {
  text-align: start;
  color: var(--p-text-muted-color);
  padding-bottom: 0.25rem;
}

th,
td {
  text-align: start;
  padding: 0.375rem 0.5rem;
  border-bottom: 1px solid var(--p-content-border-color);
}

tbody th {
  font-weight: normal;
}
</style>
