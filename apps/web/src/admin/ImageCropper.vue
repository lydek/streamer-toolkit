<template>
  <!-- The picked rectangle over the picture: drag it to move, drag a corner to resize (the opposite corner stays);
       arrows move it, Shift with arrows resizes it. Outside it is dimmed. `ratio` keeps width ÷ height. -->
  <div class="cropper">
    <img :src="src" alt="" draggable="false" />
    <div
      class="frame"
      tabindex="0"
      role="group"
      :aria-label="t('imageSizes.cropLabel', { x: crop.x, y: crop.y, w: crop.w, h: crop.h })"
      :aria-describedby="hintId"
      :style="frameStyle"
      @pointerdown="start($event, 'move')"
      @keydown="key"
      @keyup="emit('commit')"
    >
      <span
        v-for="c in CORNERS"
        :key="c"
        class="handle"
        :class="c"
        aria-hidden="true"
        @pointerdown.stop="start($event, c)"
      ></span>
    </div>
    <p :id="hintId" class="sr-only">{{ t('imageSizes.cropKeys') }}</p>
  </div>
</template>

<script setup lang="ts">
import { computed, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Corner, type Crop, moveCrop, resizeCrop } from './imageSizes'

/** `width` and `height`: the picture's own size in pixels, which `crop` is counted in. */
const props = defineProps<{ src: string; width: number; height: number; ratio?: number }>()
const CORNERS: Corner[] = ['nw', 'ne', 'sw', 'se']
const crop = defineModel<Crop>({ required: true })
const emit = defineEmits<{ commit: [] }>()
const { t } = useI18n()
const hintId = useId()

const pct = (n: number, of: number) => `${(n / of) * 100}%`
const frameStyle = computed(() => ({
  left: pct(crop.value.x, props.width),
  top: pct(crop.value.y, props.height),
  width: pct(crop.value.w, props.width),
  height: pct(crop.value.h, props.height),
}))

/** Pointer drags: the frame moves, a corner resizes; distances are turned into the picture's pixels. */
function start(e: PointerEvent, mode: 'move' | Corner) {
  const box = (e.currentTarget as HTMLElement).closest('.cropper')!.getBoundingClientRect()
  const scale = props.width / box.width
  const from = { x: e.clientX, y: e.clientY, crop: crop.value }
  const target = e.currentTarget as HTMLElement
  target.setPointerCapture(e.pointerId)
  const move = (ev: PointerEvent) => {
    const dx = Math.round((ev.clientX - from.x) * scale)
    const dy = Math.round((ev.clientY - from.y) * scale)
    crop.value =
      mode === 'move'
        ? moveCrop(from.crop, dx, dy, props.width, props.height)
        : resizeCrop(from.crop, mode, dx, dy, props.width, props.height, props.ratio)
  }
  const up = () => {
    target.removeEventListener('pointermove', move)
    emit('commit')
  }
  target.addEventListener('pointermove', move)
  target.addEventListener('pointerup', up, { once: true })
}

function key(e: KeyboardEvent) {
  const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key]
  if (!d) return
  e.preventDefault()
  // A step is a twentieth of the rectangle's shorter side, at least a pixel.
  const step = Math.max(1, Math.round(Math.min(crop.value.w, crop.value.h) / 20))
  crop.value = e.shiftKey
    ? resizeCrop(crop.value, 'se', d[0] * step, d[1] * step, props.width, props.height, props.ratio)
    : moveCrop(crop.value, d[0] * step, d[1] * step, props.width, props.height)
}
</script>

<style scoped>
.cropper {
  position: relative;
  max-width: 28rem;
  overflow: hidden;
  touch-action: none;
  user-select: none;
}

.cropper img {
  display: block;
  width: 100%;
}

.frame {
  position: absolute;
  box-sizing: border-box;
  border: 2px solid #fff;
  /* Dims everything outside the square; the box clips it. */
  box-shadow: 0 0 0 100vmax rgb(0 0 0 / 55%);
  cursor: move;
}

.frame:focus-visible {
  outline: 2px solid var(--p-primary-color);
  outline-offset: 2px;
}

.handle {
  position: absolute;
  width: 1rem;
  height: 1rem;
  border: 2px solid #fff;
  border-radius: 50%;
  background: var(--p-primary-color);
}

.handle.nw {
  top: -0.5rem;
  left: -0.5rem;
  cursor: nwse-resize;
}

.handle.ne {
  top: -0.5rem;
  right: -0.5rem;
  cursor: nesw-resize;
}

.handle.sw {
  bottom: -0.5rem;
  left: -0.5rem;
  cursor: nesw-resize;
}

.handle.se {
  right: -0.5rem;
  bottom: -0.5rem;
  cursor: nwse-resize;
}
</style>
