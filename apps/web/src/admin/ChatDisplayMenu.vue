<template>
  <!-- 顯示: how chat lines look in the admin UI (saved, shared by every chat view). -->
  <div ref="root" class="display-menu" :class="{ compact: iconOnly, floating: popover && !iconOnly }">
    <Button
      ref="trigger"
      :label="iconOnly ? undefined : t('chat.display.title')"
      :aria-label="iconOnly ? t('chat.display.title') : undefined"
      icon="pi pi-sliders-h"
      :size="iconOnly ? undefined : 'small'"
      :text="iconOnly"
      :rounded="iconOnly"
      :outlined="!iconOnly"
      :aria-expanded="open"
      :aria-controls="id"
      @click="open = !open"
    />
    <div v-if="open" :id="id" class="panel" role="group" :aria-label="t('chat.display.title')">
      <span :id="`${id}-style`">{{ t('chat.display.style') }}</span>
      <SelectButton
        :model-value="chatDisplay.style"
        :options="STYLES"
        :option-label="(s: Style) => t(`chat.display.styles.${s}`)"
        :allow-empty="false"
        :aria-labelledby="`${id}-style`"
        @update:model-value="(style: Style) => save({ style })"
      />
      <span v-for="k in TOGGLES" :key="k" class="check">
        <Checkbox :model-value="chatDisplay[k]" :input-id="`${id}-${k}`" binary @update:model-value="(v: boolean) => save({ [k]: v })" />
        <label :for="`${id}-${k}`">{{ t(`chat.display.${k}`) }}</label>
      </span>
      <label :for="`${id}-size`">{{ t('chat.display.textSize') }}</label>
      <Select
        :model-value="chatDisplay.textSize"
        :input-id="`${id}-size`"
        :options="TEXT_SIZES.map((n) => ({ value: n, label: `${n}%` }))"
        option-label="label"
        option-value="value"
        @update:model-value="(textSize: number) => save({ textSize })"
      />
      <!-- Only in the docked panel: which side of the pages it sits on (wide windows). -->
      <template v-if="dockSide">
        <span :id="`${id}-side`">{{ t('chat.display.side') }}</span>
        <SelectButton
          :model-value="dock.side"
          :options="SIDES"
          :option-label="(s: Side) => t(`chat.display.sides.${s}`)"
          :allow-empty="false"
          :aria-labelledby="`${id}-side`"
          @update:model-value="setSide"
        />
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import Select from 'primevue/select'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import { nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas } from './api'
import { type ChatDisplay, TEXT_SIZES, chatDisplay, saveChatDisplay } from './chatDisplay'
import { saveDock, useDock } from './dock'
import { errorToast } from './home'

type Style = ChatDisplay['style']
type Side = Schemas['DockSide']
const SIDES: Side[] = ['left', 'right']
defineProps<{ dockSide?: boolean; iconOnly?: boolean; popover?: boolean }>()
const STYLES: Style[] = ['compact', 'standard', 'comfortable']
const TOGGLES = ['avatars', 'badges', 'time'] as const

const { t } = useI18n()
const toast = useToast()
const id = useId()
const open = ref(false)
const root = ref<HTMLElement>()
const trigger = ref<{ $el: HTMLElement }>()

function onKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape' || !open.value) return
  e.preventDefault()
  e.stopPropagation()
  open.value = false
  void nextTick(() => trigger.value?.$el.focus())
}

function onPointerdown(e: PointerEvent) {
  const target = e.target as Node | null
  if (!open.value || !target || root.value?.contains(target)) return
  // PrimeVue Select dropdowns teleport to body: clicking an option inside one stays inside the menu.
  if (target instanceof Element && target.closest('.p-select-overlay')) return
  open.value = false
}

watch(open, (isOpen) => {
  if (isOpen) {
    document.addEventListener('keydown', onKeydown, true)
    document.addEventListener('pointerdown', onPointerdown)
  } else {
    document.removeEventListener('keydown', onKeydown, true)
    document.removeEventListener('pointerdown', onPointerdown)
  }
})

onBeforeUnmount(() => {
  document.removeEventListener('keydown', onKeydown, true)
  document.removeEventListener('pointerdown', onPointerdown)
})

const dock = useDock()
async function setSide(side: Side) {
  const before = dock.side
  dock.side = side
  try {
    await saveDock()
  } catch (e) {
    dock.side = before
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  }
}

async function save(change: Partial<ChatDisplay>) {
  try {
    await saveChatDisplay(change)
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  }
}
</script>

<style scoped>
.display-menu {
  display: grid;
  gap: 0.5rem;
}

.display-menu.compact {
  display: inline-flex;
}

.display-menu.floating {
  position: relative;
  display: inline-flex;
}

.display-menu.compact .panel,
.display-menu.floating .panel {
  position: absolute;
  top: calc(100% + 0.25rem);
  right: 0.5rem;
  z-index: 10;
  background: var(--p-surface-900);
  box-shadow: 0 0.5rem 1.5rem rgb(0 0 0 / 0.5);
}

.display-menu.compact .panel {
  left: 0.5rem;
}

.display-menu.floating .panel {
  right: 0;
  width: min(22rem, 90vw);
}

/* Narrow toolbar: anchor full-width to the toolbar instead of overhanging past the left edge. */
@container view (max-width: 45rem) {
  .display-menu.floating {
    position: static;
  }

  .display-menu.floating .panel {
    left: 0.5rem;
    right: 0.5rem;
    width: auto;
  }
}

.panel {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
  padding: 0.5rem;
  border: 1px solid var(--p-surface-700);
  border-radius: 6px;
}

.check {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
}
</style>
