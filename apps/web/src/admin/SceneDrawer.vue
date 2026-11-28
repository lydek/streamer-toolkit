<template>
  <AppDrawer id="scene" :title="title" full-page="/scenes">
    <template #actions>
      <DrawerNotice />
    </template>
    <template #now>
      <template v-if="obsReady(live.obsState)">
        <SceneList :state="live.obsState" @error="failed" />
        <!-- The edited scene's items (the preview in studio mode, as on the scene page): show or hide each. -->
        <template v-if="current">
          <h3 class="items-head">{{ t('scene.items', { scene: current.name }) }}</h3>
          <p v-if="!current.items.length" class="muted">{{ t('scene.noItems') }}</p>
          <ul v-else :key="resets" class="items">
            <li v-for="item in current.items" :key="item.id">
              <i :class="kindIcon(item.kind)" class="kind" aria-hidden="true"></i>
              <span class="name">{{ item.source }}</span>
              <ToggleSwitch
                :model-value="item.enabled"
                :disabled="busy.has(item.id)"
                :aria-label="t('scene.showItem', { item: item.source })"
                @update:model-value="(v: boolean) => show(current!.name, item.id, v)"
              />
            </li>
          </ul>
        </template>
      </template>
      <p v-else>{{ t('scene.notConnected') }}</p>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import ToggleSwitch from 'primevue/toggleswitch'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import type { LiveState } from './home'
import { useHomeDrawer } from './homeDrawer'
import SceneList from './SceneList.vue'
import { kindIcon, obsReady, updateItem } from './scenes'

const props = defineProps<{ title: string; live: LiveState }>()
const { t } = useI18n()
const { failed } = useHomeDrawer()
const current = computed(() => {
  const obs = props.live.obsState
  if (!obsReady(obs)) return undefined
  const target = obs.studioMode && obs.preview ? obs.preview : obs.scene
  return obs.scenes.find((s) => s.name === target)
})
const busy = ref(new Set<number>())
// A refused change redraws the switches from OBS's state.
const resets = ref(0)

async function show(scene: string, item: number, enabled: boolean) {
  busy.value = new Set(busy.value).add(item)
  try {
    await updateItem(scene, item, { enabled })
  } catch (e) {
    resets.value++
    failed(e)
  } finally {
    const next = new Set(busy.value)
    next.delete(item)
    busy.value = next
  }
}
</script>

<style scoped>
.items-head {
  margin: 1rem 0 0.5rem;
  font-size: 0.9375rem;
}

.items {
  display: grid;
  gap: 0.25rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.items li {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.kind {
  color: var(--p-text-muted-color);
}

.name {
  min-width: 0;
  margin-right: auto;
  overflow-wrap: anywhere;
}
</style>
