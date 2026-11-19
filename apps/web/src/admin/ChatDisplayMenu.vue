<template>
  <!-- 顯示: how chat lines look in the admin UI (saved, shared by every chat view). -->
  <div class="display-menu">
    <Button
      :label="t('chat.display.title')"
      icon="pi pi-sliders-h"
      size="small"
      outlined
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
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import Select from 'primevue/select'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import { ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import { type ChatDisplay, TEXT_SIZES, chatDisplay, saveChatDisplay } from './chatDisplay'
import { errorToast } from './home'

type Style = ChatDisplay['style']
const STYLES: Style[] = ['compact', 'standard', 'comfortable']
const TOGGLES = ['avatars', 'badges', 'time'] as const

const { t } = useI18n()
const toast = useToast()
const id = useId()
const open = ref(false)

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
