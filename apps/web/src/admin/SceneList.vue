<template>
  <!-- OBS's scenes as buttons; the one on air is pressed. Shared by the scene page and its drawer. -->
  <ul class="plain scenes">
    <li v-for="s in state.scenes" :key="s.name">
      <Button
        :label="s.name"
        :outlined="s.name !== state.scene"
        :aria-pressed="s.name === state.scene"
        :disabled="busy"
        @click="choose(s.name)"
      />
      <span v-if="s.name === state.scene" class="tag">{{ t('scene.onAir') }}</span>
    </li>
  </ul>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type StreamingState, switchScene } from './scenes'

const props = defineProps<{ state: StreamingState & { scene: string } }>()
const emit = defineEmits<{ error: [e: unknown] }>()
const { t } = useI18n()
const busy = ref(false)

async function choose(scene: string) {
  if (scene === props.state.scene) return
  busy.value = true
  try {
    await switchScene(scene)
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
</style>
