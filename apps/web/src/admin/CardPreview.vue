<template>
  <section class="previews" :aria-label="t('cardStyle.preview')">
    <figure v-for="v in VARIANTS" :key="v">
      <!-- The overlay never renders in the admin window: it runs in a sandbox without allow-same-origin (origin
           `null`, no IPC, no token) and gets its sample card by postMessage. -->
      <iframe
        ref="frames"
        class="frame"
        sandbox="allow-scripts"
        tabindex="-1"
        src="/overlay/checkin/?preview"
        :title="t(`cardStyle.previews.${v}`)"
        :data-variant="v"
        @load="post($event.target as HTMLIFrameElement)"
      ></iframe>
      <figcaption>{{ t(`cardStyle.previews.${v}`) }}</figcaption>
    </figure>
  </section>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Schemas } from './api'
import { previewCard } from './cardStyle'

const VARIANTS = ['stamped', 'repeat'] as const

const props = defineProps<{ style: Schemas['CardStyleBody']; slots: number }>()
const { t } = useI18n()
const frames = ref<HTMLIFrameElement[]>([])
// Each message needs a newer seq than the last, or the overlay takes it for a replay.
let seq = 0

// The iframes' origin is `null`, which no targetOrigin can name; what goes in is the broadcaster's own sample only.
function post(frame: HTMLIFrameElement) {
  const repeat = frame.dataset.variant === 'repeat'
  const target = frame.contentWindow
  if (!target) return
  target.postMessage({ type: 'snapshot', version: 1, seq: ++seq, payload: {} }, '*')
  target.postMessage(
    {
      id: `preview-${seq}`,
      type: 'checkin.stamped',
      version: 1,
      seq: ++seq,
      occurredAt: new Date().toISOString(),
      source: 'simulation',
      payload: previewCard(props.style, props.slots, repeat),
    },
    '*',
  )
}

watch(
  () => [props.style, props.slots],
  () => frames.value.forEach(post),
  { deep: true },
)
</script>

<style scoped>
.previews {
  display: grid;
  gap: 1rem;
  align-content: start;
}

figure {
  margin: 0;
}

.frame {
  width: 100%;
  aspect-ratio: 16 / 9;
  border: 0;
  border-radius: 6px;
  background: var(--p-surface-800);
  pointer-events: none;
}

figcaption {
  margin-top: 0.25rem;
  color: var(--p-text-muted-color);
}
</style>
