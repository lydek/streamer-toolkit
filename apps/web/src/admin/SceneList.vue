<template>
  <!-- OBS's scenes as buttons; the one picked is pressed: the one on air, or in studio mode the one in the preview.
       Shared by the scene page (`list`: one row per scene, as the prototype) and its drawer (a row of buttons). -->
  <ul v-if="list" class="scene-list">
    <li v-for="s in state.scenes" :key="s.name">
      <button
        type="button"
        :class="{ live: s.name === state.scene, preview: state.studioMode && s.name === state.preview }"
        :aria-pressed="s.name === picked"
        :disabled="busy"
        @click="choose(s.name)"
      >
        <span class="thumb" aria-hidden="true"></span>
        <span class="name">{{ s.name }}</span>
        <span v-if="s.name === state.scene" class="stag live">{{ t('scene.onAir') }}</span>
        <span v-if="state.studioMode && s.name === state.preview" class="stag preview">{{ t('scene.preview') }}</span>
      </button>
    </li>
  </ul>
  <ul v-else class="scenes">
    <li v-for="s in state.scenes" :key="s.name">
      <Button
        :label="s.name"
        :outlined="s.name !== picked"
        :aria-pressed="s.name === picked"
        :disabled="busy"
        @click="choose(s.name)"
      />
      <span v-if="s.name === state.scene" class="tag">{{ t('scene.onAir') }}</span>
      <span v-if="state.studioMode && s.name === state.preview" class="tag preview">{{ t('scene.preview') }}</span>
    </li>
  </ul>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type StreamingState, previewScene, switchScene } from './scenes'

const props = defineProps<{ state: StreamingState & { scene: string }; list?: boolean }>()
const emit = defineEmits<{ error: [e: unknown] }>()
const { t } = useI18n()
const busy = ref(false)
const picked = computed(() => (props.state.studioMode ? props.state.preview : props.state.scene))

async function choose(scene: string) {
  if (scene === picked.value) return
  busy.value = true
  try {
    await (props.state.studioMode ? previewScene(scene) : switchScene(scene))
  } catch (e) {
    emit('error', e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
/* Its own list reset: the parent's styles never reach a child's root (that left the scene page's list indented). */
.scenes,
.scene-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.scenes {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.scene-list {
  display: grid;
  gap: 0.375rem;
}

.scene-list button {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  width: 100%;
  padding: 0.375rem 0.5rem;
  border: 2px solid transparent;
  border-radius: 6px;
  background: var(--p-surface-950);
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}

.scene-list button:hover:not(:disabled) {
  background: var(--p-surface-800);
}

.scene-list button.live {
  border-color: var(--p-red-400);
}

.scene-list button.preview {
  border-color: var(--p-green-400);
}

.scene-list button[aria-pressed='true'] {
  font-weight: 600;
}

.thumb {
  flex: none;
  width: 3rem;
  height: 1.6875rem;
  border-radius: 4px;
  background: var(--p-surface-700);
}

.name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stag {
  flex: none;
  padding: 0 0.5rem;
  border-radius: 999px;
  font-size: 0.75rem;
}

.scene-list .stag:first-of-type {
  margin-left: auto;
}

/* The same pills as the item table's heading on the scene page. */
.stag.live {
  background: color-mix(in srgb, var(--p-red-400) 20%, transparent);
  color: var(--p-red-300);
}

.stag.preview {
  background: color-mix(in srgb, var(--p-green-400) 20%, transparent);
  color: var(--p-green-300);
}

.scenes li {
  display: flex;
  align-items: center;
  gap: 0.25rem;
}

.tag {
  font-size: 0.8125rem;
  color: var(--p-primary-color);
}

.tag.preview {
  color: var(--p-green-400);
}
</style>
