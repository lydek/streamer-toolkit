<template>
  <!-- OBS's scenes as buttons; the one picked is pressed: the one on air, or in studio mode the one in the preview.
       Shared by the scene page and its drawer. -->
  <ul class="plain scenes">
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

const props = defineProps<{ state: StreamingState & { scene: string } }>()
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
.scenes {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
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
