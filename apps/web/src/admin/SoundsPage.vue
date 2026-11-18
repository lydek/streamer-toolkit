<template>
  <FullPage :title="t('modules.names.sound')">
    <div class="stack">
      <label for="sound-file">{{ t('sound.file') }}</label>
      <input
        id="sound-file"
        ref="input"
        type="file"
        accept=".mp3,.wav,.ogg,audio/mpeg,audio/wav,audio/ogg"
        :disabled="busy"
        @change="choose"
      />
      <p v-if="importError" role="alert">{{ importError }}</p>
    </div>

    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!library">{{ t('app.loading') }}</p>
    <p v-else-if="library.sounds.length === 0">{{ t('sound.empty') }}</p>
    <ul v-else class="sounds">
      <SoundRow
        v-for="s in library.sounds"
        :key="s.id"
        :sound="s"
        :used-by="s.used_by.map((slot) => slotLabel(slot, t, rewardTitles))"
        @deleted="deleted"
        @error="report"
      />
    </ul>

    <section class="stack" aria-labelledby="reward-sounds">
      <h2 id="reward-sounds">{{ t('sound.rewards') }}</h2>
      <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined class="copy" @click="copyObsUrl" />
      <p v-if="rewardError">{{ rewardError }}</p>
      <SoundSlots
        v-if="library && rewardSlots.length"
        :key="`${version}:${rewardSlots.map((r) => r.slot)}`"
        :slots="rewardSlots"
        playable
        @played="toast.add(successToast(t('sound.playedInObs')))"
        @error="report"
      />
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'
import { keepFocus } from './page'
import { importSound, measureUnmeasured, slotLabel } from './sounds'
import SoundRow from './SoundRow.vue'
import SoundSlots from './SoundSlots.vue'

const { t } = useI18n()
const toast = useToast()
const input = ref<HTMLInputElement>()
const library = shallowRef<Schemas['SoundLibrary']>()
const failed = ref('')
const importError = ref('')
const busy = ref(false)
// Bumped on every load, so the reward choices list the current sounds.
const version = ref(0)
const rewards = ref<Schemas['Reward'][]>([])
const rewardError = ref('')
const rewardTitles = computed(() => new Map(rewards.value.map((r) => [r.id, r.title])))
// The channel's rewards, plus rewards with a sound that Twitch no longer lists (shown by ID).
const rewardSlots = computed(() => {
  const ids = [
    ...rewards.value.map((r) => r.id),
    ...Object.keys(library.value?.slots ?? {}).flatMap((s) => (s.startsWith('reward.') ? [s.slice(7)] : [])),
  ]
  return [...new Set(ids)].map((id) => ({ slot: `reward.${id}`, label: rewardTitles.value.get(id) ?? id }))
})

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))

async function load() {
  try {
    library.value = await api(client.GET('/api/admin/sounds'))
    version.value++
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : t('sound.loadFailed')
  }
}

async function choose() {
  importError.value = ''
  const file = input.value?.files?.[0]
  if (!file) return
  busy.value = true
  try {
    const sound = await importSound(file)
    toast.add(successToast(t('sound.imported', { label: sound.label })))
    // Its length decides how long it holds the presentation queue; a failure is retried when the window opens.
    void measureUnmeasured().catch(() => {})
    await load()
  } catch (e) {
    importError.value = message(e)
  } finally {
    busy.value = false
    if (input.value) input.value.value = ''
  }
}

async function deleted(s: Schemas['Sound']) {
  toast.add(successToast(t('sound.deleted', { label: s.label })))
  await load()
  // The deleted row took the focused button with it.
  await keepFocus(() => input.value)
}

const report = (e: unknown) => toast.add(errorToast(message(e)))

async function copyObsUrl() {
  try {
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/sound/`)
    toast.add(successToast(t('chat.copied')))
  } catch {
    report(t('chat.copyFailed'))
  }
}

async function loadRewards() {
  try {
    rewards.value = await api(client.GET('/api/admin/checkin/rewards'))
  } catch (e) {
    rewardError.value = message(e)
  }
}

void load()
void loadRewards()
</script>

<style scoped>
.copy {
  align-self: flex-start;
}

.sounds {
  list-style: none;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
</style>
