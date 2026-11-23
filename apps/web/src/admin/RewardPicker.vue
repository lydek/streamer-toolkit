<template>
  <!-- Next to the chat box: the channel's rewards, each with 播放效果 (only the local effect it is set to play:
       its sound and its video or GIF) and 模擬兌換 (that effect, plus the check-in stamp card when it is the check-in
       reward, all as `source=simulation`). No points are spent and nothing is recorded. -->
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
      <template v-else>
      <label :for="`${panelId}-viewer`" class="viewer-label">{{ t('dock.rewards.viewer') }}</label>
      <InputText :id="`${panelId}-viewer`" v-model="viewer" size="small" maxlength="25" :placeholder="t('dock.rewards.viewerHint')" class="viewer" />
      <ul class="list">
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
          <Button
            :label="t('dock.rewards.simulate')"
            :aria-label="t('dock.rewards.simulateLabel', { title: r.title })"
            size="small"
            text
            :disabled="busy"
            @click="simulate(r)"
          />
        </li>
      </ul>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { nextTick, ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast, successToast } from './home'
import { simulateCheckin, testSound } from './simulate'

const { t } = useI18n()
const toast = useToast()
const panelId = useId()
const open = ref(false)
const busy = ref(false)
const failed = ref(false)
const rewards = ref<Schemas['Reward'][]>()
const slots = ref<Record<string, string>>({})
const viewer = ref('')
// The check-in reward, while check-ins are on (a simulated redemption of it shows a stamp card).
const checkinReward = ref<string | null>(null)

// Read on every open: rewards, sound slots and the check-in reward change on other pages while the dock stays.
async function toggle() {
  open.value = !open.value
  if (!open.value) return
  void keepInView()
  failed.value = false
  try {
    const [list, library, modules, checkin] = await Promise.all([
      api(client.GET('/api/admin/checkin/rewards')),
      api(client.GET('/api/admin/sounds')),
      api(client.GET('/api/admin/modules')),
      api(client.GET('/api/admin/checkin/settings')).catch(() => null),
    ])
    rewards.value = list
    slots.value = library.slots
    const checkinOn = modules.some((m) => m.id === 'checkin' && m.enabled)
    checkinReward.value = checkinOn ? (checkin?.reward_id ?? null) : null
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

/** Plays the reward's sound (when it has one) and its video or GIF (when it has a rule); whether anything played. */
async function effect(r: Schemas['Reward']) {
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
  return played
}

async function act(r: Schemas['Reward'], run: () => Promise<string | null>) {
  busy.value = true
  try {
    const done = await run()
    toast.add(done ? successToast(done) : { severity: 'info', summary: t('dock.rewards.none', { title: r.title }), life: 3000 })
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = false
  }
}

const play = (r: Schemas['Reward']) => act(r, async () => ((await effect(r)) ? t('dock.rewards.played', { title: r.title }) : null))

/** The supported simulated parts of a redemption: its effect, and the stamp card for the check-in reward. */
const simulate = (r: Schemas['Reward']) =>
  act(r, async () => {
    let done = await effect(r)
    if (r.id === checkinReward.value) {
      await simulateCheckin({ stamps: 1, userName: viewer.value.trim() || undefined })
      done = true
    }
    return done ? t('dock.rewards.simulated', { title: r.title }) : null
  })
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

.viewer-label {
  display: block;
  margin-bottom: 0.25rem;
  font-size: 0.8125rem;
  color: var(--p-text-muted-color);
}

.viewer {
  width: 100%;
  margin-bottom: 0.5rem;
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
