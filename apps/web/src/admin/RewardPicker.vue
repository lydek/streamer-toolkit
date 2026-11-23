<template>
  <!-- Next to the chat box: the channel's rewards, each with 播放效果 (only the local effect it is set to play:
       its sound and its video or GIF; no points spent, no record written). -->
  <div class="reward-picker">
    <Button
      icon="pi pi-star"
      text
      rounded
      :aria-label="t('dock.rewards.open')"
      :aria-expanded="open"
      :aria-controls="panelId"
      @click="toggle"
    />
    <div v-if="open" :id="panelId" ref="panel" class="panel" :style="{ left: `${shift}px` }" role="group" :aria-label="t('dock.rewards.title')">
      <p v-if="failed" role="alert">{{ t('dock.rewards.loadFailed') }}</p>
      <p v-else-if="!rewards">{{ t('app.loading') }}</p>
      <p v-else-if="!rewards.length">{{ t('dock.rewards.empty') }}</p>
      <ul v-else class="list">
        <li v-for="r in rewards" :key="r.id">
          <span class="title">{{ r.title }}</span>
          <Button
            :label="t('dock.rewards.play')"
            :aria-label="t('dock.rewards.playLabel', { title: r.title })"
            size="small"
            outlined
            :disabled="busy"
            @click="play(r)"
          />
        </li>
      </ul>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { nextTick, ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast, successToast } from './home'
import { testSound } from './simulate'

const { t } = useI18n()
const toast = useToast()
const panelId = useId()
const open = ref(false)
const busy = ref(false)
const failed = ref(false)
const rewards = ref<Schemas['Reward'][]>()
const slots = ref<Record<string, string>>({})

// Read on every open: rewards, sound slots and the check-in reward change on other pages while the dock stays.
async function toggle() {
  open.value = !open.value
  if (!open.value) return
  void keepInView()
  failed.value = false
  try {
    const [list, library] = await Promise.all([api(client.GET('/api/admin/checkin/rewards')), api(client.GET('/api/admin/sounds'))])
    rewards.value = list
    slots.value = library.slots
  } catch {
    failed.value = true
  }
}

// The panel opens from the button's left edge; a narrow chat panel at the window's right edge moves it left to fit.
const panel = ref<HTMLElement>()
const shift = ref(0)
async function keepInView() {
  shift.value = 0
  await nextTick()
  const box = panel.value?.getBoundingClientRect()
  if (box) shift.value = Math.min(0, window.innerWidth - 8 - box.right)
}

/** The reward's sound (when it has one) and its video or GIF (when it has a rule); says when it has neither. */
async function play(r: Schemas['Reward']) {
  busy.value = true
  try {
    const sound = slots.value[`reward.${r.id}`]
    let played = false
    if (sound && sound !== 'silent') {
      await testSound(sound)
      played = true
    }
    try {
      await api(client.POST('/api/admin/screen-tools/media-rules/{reward_id}/test', { params: { path: { reward_id: r.id } } }))
      played = true
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'media.not_found')) throw e
    }
    toast.add(played ? successToast(t('dock.rewards.played', { title: r.title })) : { severity: 'info', summary: t('dock.rewards.none', { title: r.title }), life: 3000 })
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.reward-picker {
  position: relative;
}

.panel {
  position: absolute;
  left: 0;
  bottom: calc(100% + 0.375rem);
  z-index: 3;
  width: min(20rem, calc(100vw - 2rem));
  max-height: 20rem;
  overflow-y: auto;
  padding: 0.75rem;
  border: 1px solid var(--p-surface-700);
  border-radius: 6px;
  background: var(--p-surface-900);
  box-shadow: 0 0.5rem 1.5rem rgb(0 0 0 / 0.5);
}

.panel p {
  margin: 0;
}

.list {
  display: grid;
  gap: 0.375rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.list li {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.title {
  flex: 1;
  min-width: 0;
}
</style>
