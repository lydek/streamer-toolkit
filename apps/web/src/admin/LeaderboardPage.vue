<template>
  <!-- The four boards side by side (one column when narrow), each saying which of its periods are in OBS. -->
  <FullPage :title="t('pages.leaderboard.name')">
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
    <p v-if="period === 'all'" class="note">{{ t('leaderboard.allKept') }}</p>

    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!boards">{{ t('app.loading') }}</p>
    <div v-else class="boards">
      <section v-for="k in KINDS" :key="k" class="board" :aria-labelledby="`lb-${k}`">
        <h2 :id="`lb-${k}`">{{ t(`overlay.leaderboard.${k}`) }}</h2>
        <RouterLink to="/obs-sources" class="in-obs">
          {{ inObs(k).length ? t('leaderboard.inObs', { periods: inObs(k).join('、') }) : t('obsStatus.missing') }}
        </RouterLink>
        <p v-if="period === 'all' && k === 'checkins'" class="note">{{ t('leaderboard.allCheckins') }}</p>
        <p v-if="!entries(k).length" class="note">{{ t('leaderboard.empty') }}</p>
        <ol v-else class="entries">
          <li v-for="(e, i) in entries(k)" :key="i">
            <span>{{ e.name }}</span>
            <output>{{ e.value.toLocaleString() }}</output>
          </li>
        </ol>
        <Button
          :label="t('chat.copyObsUrl')"
          icon="pi pi-copy"
          text
          size="small"
          :aria-label="t('leaderboard.copyFor', { board: t(`overlay.leaderboard.${k}`) })"
          @click="copy(k)"
        />
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { obsPresence, useObsSources } from './obsSources'
import { errorToast, successToast, useLiveEvents } from './home'

type Period = Schemas['Period']
type Kind = Schemas['BoardKind']
const PERIODS: Period[] = ['stream', 'month', 'all']
const KINDS: Kind[] = ['bits', 'gifts', 'chat', 'checkins']

const { t } = useI18n()
const toast = useToast()
const period = ref<Period>('stream')
const boards = shallowRef<Schemas['LeaderboardPayload']>()
const failed = ref('')
const entries = (k: Kind) => boards.value?.boards.find((b) => b.board === k && b.period === period.value)?.entries ?? []
// Which periods of a board OBS shows (from the OBS sources, read when the page opens).
const obsSources = useObsSources(true)
const inObs = (k: Kind) =>
  PERIODS.filter((p) => {
    const presence = obsPresence(`/overlay/leaderboard/?board=${k}&period=${p}`, obsSources.value)
    return presence.kind === 'in' || presence.kind === 'duplicate'
  }).map((p) => t(`leaderboard.periods.${p}`))

async function load() {
  try {
    boards.value = await api(client.GET('/api/admin/leaderboard'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}

async function copy(k: Kind) {
  try {
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/leaderboard/?board=${k}&period=${period.value}`)
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
.boards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(16rem, 100%), 1fr));
  gap: 1rem;
  margin-top: 1rem;
}

.board {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 0.75rem 1rem;
  border: 1px solid var(--p-content-border-color);
  border-radius: 6px;
}

.board h2 {
  margin: 0;
  font-size: 1rem;
}

.board .p-button {
  align-self: flex-start;
}

.in-obs {
  font-size: 0.8125rem;
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
  margin: 0;
  padding-inline-start: 1.25rem;
}

.entries li {
  display: flex;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.25rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}
</style>
