<template>
  <FullPage tile="sound" overlay="sound" :status="library ? t('sound.count', { n: library.sounds.length }) : undefined">
    <div class="stack">
      <FilePick ref="pick" input-id="sound-file" :label="t('sound.file')" icon="pi pi-plus" accept=".mp3,.wav,.ogg,audio/mpeg,audio/wav,audio/ogg" :disabled="busy" @pick="choose" />
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

  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import FilePick from './FilePick.vue'
import { errorToast, successToast } from './home'
import { keepFocus } from './page'
import { importSound, measureUnmeasured, slotLabel } from './sounds'
import SoundRow from './SoundRow.vue'

const { t } = useI18n()
const toast = useToast()
const pick = ref<InstanceType<typeof FilePick>>()
const library = shallowRef<Schemas['SoundLibrary']>()
const failed = ref('')
const importError = ref('')
const busy = ref(false)
const rewards = ref<Schemas['Reward'][]>([])
// Reward titles for each sound's "used by"; what a reward plays is set on 兌換時播放.
const rewardTitles = computed(() => new Map(rewards.value.map((r) => [r.id, r.title])))

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))

async function load() {
  try {
    library.value = await api(client.GET('/api/admin/sounds'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : t('sound.loadFailed')
  }
}

async function choose(files: File[]) {
  importError.value = ''
  const file = files[0]
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
  }
}

async function deleted(s: Schemas['Sound']) {
  toast.add(successToast(t('sound.deleted', { label: s.label })))
  await load()
  // The deleted row took the focused button with it.
  await keepFocus(() => pick.value?.element())
}

const report = (e: unknown) => toast.add(errorToast(message(e)))

async function loadRewards() {
  // Without the list, a reward shows by its ID.
  rewards.value = await api(client.GET('/api/admin/checkin/rewards')).catch(() => [])
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
