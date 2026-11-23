<template>
  <!-- OBS's audio inputs of one scene (the live one by default): fader, mute and monitoring, applied to OBS at once. -->
  <FullPage :title="t('pages.mixer.name')">
    <p v-if="live.status === 'loading'">{{ t('app.loading') }}</p>
    <!-- OBS down: the page's notice (FullPage) says so; nothing more here. -->
    <template v-else-if="!obsReady(state) && servicesDown.obs" />
    <p v-else-if="!obsReady(state)" role="status">
      {{ t('scene.notConnected') }}
      <RouterLink to="/settings/connections#obs">{{ t('scene.connect') }}</RouterLink>
    </p>
    <section v-else class="stack" :aria-label="t('mixer.inputs')">
      <div class="mix-head">
        <label for="mix-scene">{{ t('mixer.scene') }}</label>
        <Select v-model="mixPick" input-id="mix-scene" :options="pickOptions" option-label="label" option-value="value" />
        <span v-if="scene === state.scene" class="tag">{{ t('scene.onAir') }}</span>
        <span v-else-if="state.studioMode && scene === state.preview" class="tag preview">{{ t('scene.preview') }}</span>
      </div>
      <p class="muted">{{ t('mixer.shared') }}</p>
      <h2>{{ t('mixer.inScene', { scene }) }}</h2>
      <p v-if="!sections.on.length" class="muted">{{ t('mixer.none') }}</p>
      <ul class="mix-list">
        <MixerRow v-for="i in sections.on" :key="i.name" :input="i" />
      </ul>
      <details v-if="sections.off.length" class="mix-off">
        <summary>{{ t('mixer.notInScene', { n: sections.off.length }) }}</summary>
        <ul class="mix-list">
          <MixerRow v-for="i in sections.off" :key="i.name" :input="i" />
        </ul>
      </details>
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Select from 'primevue/select'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'
import { servicesDown, useLiveEvents } from './home'
import { FOLLOW_PREVIEW, mixPick, mixScene, mixSections } from './mixer'
import MixerRow from './MixerRow.vue'
import { obsReady } from './scenes'

const { t } = useI18n()
const live = useLiveEvents()
const state = computed(() => live.value.obsState)
const scene = computed(() => (state.value ? mixScene(state.value, mixPick.value) : undefined))
const sections = computed(() => (state.value ? mixSections(state.value, scene.value) : { on: [], off: [] }))
const pickOptions = computed(() => {
  const s = state.value
  if (!s) return []
  return [
    { value: '', label: t('mixer.followLive', { scene: s.scene ?? '' }) },
    ...(s.studioMode && s.preview ? [{ value: FOLLOW_PREVIEW, label: t('mixer.followPreview', { scene: s.preview }) }] : []),
    ...s.scenes.map((x) => ({ value: x.name, label: x.name })),
  ]
})
</script>

<style scoped>
.mix-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.mix-list {
  display: grid;
  gap: 0.25rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.mix-off summary {
  cursor: pointer;
  margin-bottom: 0.5rem;
}

.tag {
  font-size: 0.8125rem;
  color: var(--p-primary-color);
}

.tag.preview {
  color: var(--p-green-400);
}
</style>
