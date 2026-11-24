<template>
  <section class="previews" :aria-label="t('cardStyle.preview')">
    <SelectButton
      v-model="active"
      :options="variantOptions"
      option-label="label"
      option-value="value"
      :allow-empty="false"
      :aria-label="t('cardStyle.previewMode')"
    />
    <!-- Both sandboxed frames stay mounted (switched via v-show) so toggling between variants does not reload
         the iframe, and neither ever leaves sandbox="allow-scripts" (origin `null`, no IPC, no token). -->
    <figure v-for="v in VARIANTS" v-show="active === v" :key="v">
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
    </figure>
  </section>
</template>

<script setup lang="ts">
import SelectButton from 'primevue/selectbutton'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Schemas } from './api'
import { previewCard } from './cardStyle'

const VARIANTS = ['stamped', 'repeat'] as const
type Variant = (typeof VARIANTS)[number]

const props = defineProps<{ style: Schemas['CardStyleBody']; slots: number }>()
const { t } = useI18n()
const active = ref<Variant>('stamped')
const variantOptions = computed(() => VARIANTS.map((value) => ({ value, label: t(`cardStyle.previews.${value}`) })))
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
  gap: 0.75rem;
  align-content: start;
}

figure {
  margin: 0;
}

.frame {
  display: block;
  width: 100%;
  aspect-ratio: 4 / 3;
  border: 0;
  border-radius: 6px;
  background: var(--p-surface-800);
  pointer-events: none;
}
</style>
