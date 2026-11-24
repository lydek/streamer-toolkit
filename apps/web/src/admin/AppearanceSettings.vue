<template>
  <FormPanel>
    <FormRow :label="t('screenTools.appearance.theme')" label-id="overlay-theme-label" top>
      <div class="themes" role="radiogroup" aria-labelledby="overlay-theme-label">
        <div
          v-for="(v, i) in THEMES"
          :key="v"
          class="theme"
          :class="{ active: theme.value.value === v, disabled: !appearance }"
        >
          <span class="tp" :style="THEME_STYLES[v]" aria-hidden="true">
            <i>{{ t('screenTools.appearance.sampleTag') }}</i>
            <b>{{ t('screenTools.appearance.sampleTitle') }}</b>
          </span>
          <button
            ref="themeBtns"
            type="button"
            role="radio"
            class="tn"
            :tabindex="theme.value.value === v ? 0 : -1"
            :aria-checked="theme.value.value === v"
            :disabled="!appearance"
            @click="pickTheme(v)"
            @keydown="onThemeKey(i, $event)"
          >
            {{ t(`screenTools.appearance.themes.${v}`) }}
          </button>
        </div>
      </div>
    </FormRow>

    <FormRow :label="t('screenTools.appearance.textSize')" input-id="overlay-text-size">
      <div class="vol">
        <input
          id="overlay-text-size"
          type="range"
          min="50"
          max="200"
          step="10"
          :value="sizeDraft ?? size.value.value"
          :disabled="!appearance"
          @input="sizeDraft = Number(($event.target as HTMLInputElement).value)"
          @change="commitSize($event)"
        />
        <output for="overlay-text-size" class="pct">{{ sizeDraft ?? size.value.value }}%</output>
      </div>
      <template #hint>{{ t('screenTools.appearance.textSizeHint') }}</template>
    </FormRow>
  </FormPanel>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Appearance } from '../overlay/appearance'
import { api, client } from './api'
import { settingQueue, useDrawerSetting } from './drawer'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'

type Theme = Appearance['theme']
const THEMES: Theme[] = ['dark', 'cream', 'contrast']
const THEME_STYLES: Record<Theme, Record<string, string>> = {
  dark: { '--bg': '#070815', '--tx': '#ffffff', '--ac': '#d8b7ff' },
  cream: { '--bg': '#fbf6ee', '--tx': '#3b2a1a', '--ac': '#8a5a2b' },
  contrast: { '--bg': '#000000', '--tx': '#ffffff', '--ac': '#ffd400' },
}

const props = defineProps<{
  /** The latest `screen.appearance`; `undefined` until the live connection has it. */
  appearance?: Appearance
}>()
const { t } = useI18n()

// As saved; PUT takes both, so each save sends the other as it is now.
const savedTheme = ref<Theme>('dark')
const savedSize = ref(100)
const sizeDraft = ref<number>()
watch(
  () => props.appearance,
  (a) => {
    if (!a) return
    savedTheme.value = a.theme
    savedSize.value = a.textSize
    sizeDraft.value = undefined
  },
  { immediate: true },
)
const save = (patch: Partial<Appearance>) =>
  api(
    client.PUT('/api/admin/screen-tools/appearance', {
      body: { theme: savedTheme.value, textSize: savedSize.value, ...patch },
    }),
  )
const queue = settingQueue()
const theme = useDrawerSetting(savedTheme, (v) => save({ theme: v }), queue)
const size = useDrawerSetting(savedSize, (v) => save({ textSize: v }), queue)

const themeBtns = ref<HTMLButtonElement[]>([])

function pickTheme(v: Theme) {
  theme.value.value = v
  void theme.commit()
}

function onThemeKey(index: number, e: KeyboardEvent) {
  const step = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0
  if (!step) return
  e.preventDefault()
  const next = (index + step + THEMES.length) % THEMES.length
  pickTheme(THEMES[next])
  themeBtns.value[next]?.focus()
}

function commitSize(e: Event) {
  const next = Number((e.target as HTMLInputElement).value)
  sizeDraft.value = undefined
  if (!Number.isFinite(next)) return
  size.value.value = next
  void size.commit()
}
</script>

<style scoped>
.themes {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(9.5rem, 100%), 1fr));
  gap: 0.75rem;
  width: 100%;
}

.theme {
  position: relative;
  display: grid;
  gap: 0.5rem;
  padding: 0.5rem;
  border: 2px solid var(--p-surface-800);
  border-radius: 6px;
  background: var(--p-surface-950);
  cursor: pointer;
}

.theme.active {
  border-color: var(--p-primary-color);
}

.theme.disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.theme:has(.tn:focus-visible) {
  outline: 2px solid var(--p-primary-color);
  outline-offset: 2px;
}

.tp {
  display: grid;
  gap: 0.125rem;
  padding: 0.625rem;
  border-radius: 4px;
  background: var(--bg);
  color: var(--tx);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--tx) 15%, transparent);
}

.tp i {
  font-style: normal;
  font-size: 0.6875rem;
  color: var(--ac);
}

.tp b {
  font-size: 0.875rem;
}

.tn {
  all: unset;
  display: block;
  font-size: 0.875rem;
  color: var(--p-text-color);
  cursor: inherit;
}

/* Stretches the radio button's hit area across the whole preview card. */
.tn::after {
  content: '';
  position: absolute;
  inset: 0;
}

.vol {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  max-width: 20rem;
  width: 100%;
}

.vol input {
  flex: 1;
  min-width: 0;
  accent-color: var(--p-primary-color);
}

.pct {
  min-width: 3.5rem;
  text-align: right;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
</style>
