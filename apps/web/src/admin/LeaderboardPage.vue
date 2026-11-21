<template>
  <FullPage :title="t('pages.leaderboard.name')">
    <div class="pickers">
      <div class="field">
        <span id="lb-period">{{ t('leaderboard.period') }}</span>
        <SelectButton
          v-model="period"
          :options="PERIODS"
          :option-label="(p: Period) => t(`leaderboard.periods.${p}`)"
          :allow-empty="false"
          aria-labelledby="lb-period"
        />
      </div>
      <div class="field">
        <span id="lb-board">{{ t('leaderboard.board') }}</span>
        <SelectButton
          v-model="kind"
          :options="KINDS"
          :option-label="(k: Kind) => t(`overlay.leaderboard.${k}`)"
          :allow-empty="false"
          aria-labelledby="lb-board"
        />
      </div>
      <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copy" />
    </div>
    <p v-if="period === 'all'" class="note">{{ t(kind === 'checkins' ? 'leaderboard.allCheckins' : 'leaderboard.allKept') }}</p>

    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!boards">{{ t('app.loading') }}</p>
    <p v-else-if="!entries.length">{{ t('leaderboard.empty') }}</p>
    <ol v-else class="entries">
      <li v-for="(e, i) in entries" :key="i">
        <span>{{ e.name }}</span>
        <output>{{ e.value.toLocaleString() }}</output>
      </li>
    </ol>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import { computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast, useLiveEvents } from './home'

type Period = Schemas['Period']
type Kind = Schemas['BoardKind']
const PERIODS: Period[] = ['stream', 'month', 'all']
const KINDS: Kind[] = ['bits', 'gifts', 'chat', 'checkins']

const { t } = useI18n()
const toast = useToast()
const period = ref<Period>('stream')
const kind = ref<Kind>('chat')
const boards = shallowRef<Schemas['LeaderboardPayload']>()
const failed = ref('')
const entries = computed(
  () => boards.value?.boards.find((b) => b.board === kind.value && b.period === period.value)?.entries ?? [],
)

async function load() {
  try {
    boards.value = await api(client.GET('/api/admin/leaderboard'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}

async function copy() {
  try {
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/leaderboard/?board=${kind.value}&period=${period.value}`)
    toast.add(successToast(t('chat.copied')))
  } catch {
    toast.add(errorToast(t('chat.copyFailed')))
  }
}

void load()
// The Host works the boards out again every 30 s; follow it.
useLiveEvents(2000, (msg) => {
  if (msg.type === 'leaderboard.update') boards.value = msg.payload
  // A reconnect's snapshot holds the boards as they are now, which may have changed while it was cut off.
  if (msg.type === 'snapshot') {
    const latest = msg.payload['leaderboard.update']?.at(-1)
    if (latest) boards.value = latest.payload
  }
})
</script>

<style scoped>
.pickers {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.75rem 1.5rem;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.note {
  color: var(--p-text-muted-color);
}

.entries {
  display: grid;
  gap: 0.25rem;
  max-width: 28rem;
}

.entries li {
  display: flex;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.25rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}
</style>
