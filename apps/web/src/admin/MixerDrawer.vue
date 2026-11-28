<template>
  <!-- 音訊混音, compact: the live scene's inputs with level, fader and mute (monitoring is on its page). -->
  <AppDrawer id="mixer" :title="title" full-page="/mixer">
    <template #now>
      <template v-if="obsReady(live.obsState)">
        <p>
          <span class="tag">{{ t('scene.onAir') }}</span> <b>{{ live.obsState.scene }}</b>
        </p>
        <p v-if="!mixLive.length">{{ t('mixer.none') }}</p>
        <ul class="mix-list">
          <MixerRow v-for="i in mixLive" :key="i.name" :input="i" :level="meters.get(i.name)" :mic="i.name === mic" compact />
        </ul>
      </template>
      <p v-else>{{ t('scene.notConnected') }}</p>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import { useDrawer } from './drawer'
import type { LiveState } from './home'
import { mixSections, useMeters, useMic } from './mixer'
import MixerRow from './MixerRow.vue'
import { obsReady } from './scenes'

const props = defineProps<{ title: string; live: LiveState }>()
const { t } = useI18n()
const drawer = useDrawer()
const mic = useMic()
const mixLive = computed(() => (obsReady(props.live.obsState) ? mixSections(props.live.obsState, props.live.obsState.scene).on : []))
// Levels only while this drawer is open.
const meters = useMeters(computed(() => drawer.current.value === 'mixer' && obsReady(props.live.obsState)))
</script>

<style scoped>
.mix-list {
  display: grid;
  gap: 0.25rem;
  padding: 0;
  list-style: none;
}

.tag {
  font-size: 0.8125rem;
  color: var(--p-primary-color);
}
</style>
