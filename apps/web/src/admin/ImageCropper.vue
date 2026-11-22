<template>
  <!-- The picked square over the picture: drag it to move, drag its corner to resize; arrows move it, Shift with
       arrows resizes it. Outside the square is dimmed. Changes are announced when a drag or key ends. -->
  <div class="cropper">
    <img :src="src" alt="" draggable="false" />
    <div
      class="frame"
      tabindex="0"
      role="group"
      :aria-label="t('imageSizes.cropLabel', { x: crop.x, y: crop.y, side: crop.side })"
      :aria-describedby="hintId"
      :style="frameStyle"
      @pointerdown="start($event, 'move')"
      @keydown="key"
      @keyup="emit('commit')"
    >
      <span class="handle" aria-hidden="true" @pointerdown.stop="start($event, 'resize')"></span>
    </div>
    <p :id="hintId" class="sr-only">{{ t('imageSizes.cropKeys') }}</p>
  </div>
</template>

<script setup lang="ts">
import { computed, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Crop, moveCrop, resizeCrop } from './imageSizes'

/** `width` and `height`: the picture's own size in pixels, which `crop` is counted in. */
const props = defineProps<{ src: string; width: number; height: number }>()
const crop = defineModel<Crop>({ required: true })
const emit = defineEmits<{ commit: [] }>()
const { t } = useI18n()
const hintId = useId()

const pct = (n: number, of: number) => `${(n / of) * 100}%`
const frameStyle = computed(() => ({
  left: pct(crop.value.x, props.width),
  top: pct(crop.value.y, props.height),
  width: pct(crop.value.side, props.width),
  height: pct(crop.value.side, props.height),
}))

/** Pointer drags: the frame moves, its corner resizes; distances are turned into the picture's pixels. */
function start(e: PointerEvent, mode: 'move' | 'resize') {
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
        : resizeCrop(from.crop, Math.max(dx, dy), props.width, props.height)
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
  // A step is a twentieth of the square, at least a pixel.
  const step = Math.max(1, Math.round(crop.value.side / 20))
  crop.value = e.shiftKey
    ? resizeCrop(crop.value, (d[0] + d[1]) * step, props.width, props.height)
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
  right: -0.5rem;
  bottom: -0.5rem;
  width: 1rem;
  height: 1rem;
  border: 2px solid #fff;
  border-radius: 50%;
  background: var(--p-primary-color);
  cursor: nwse-resize;
}
</style>
