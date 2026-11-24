<template>
  <!-- The four boards side by side (one column when narrow), each saying which of its periods are in OBS. -->
  <FullPage :status="t('leaderboard.summary')">
    <div class="period-bar">
      <span id="lb-period" class="sr-only">{{ t('leaderboard.period') }}</span>
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
        <div class="board-head">
          <h2 :id="`lb-${k}`">{{ t(`overlay.leaderboard.${k}`) }}</h2>
          <RouterLink to="/obs-sources" class="in-obs" :class="inObs(k).length ? 'tint-green' : 'tint-muted'">
            {{ inObs(k).length ? t('leaderboard.inObs', { periods: inObs(k).join('、') }) : t('obsStatus.missing') }}
          </RouterLink>
        </div>
        <p v-if="period === 'all' && k === 'checkins'" class="note">{{ t('leaderboard.allCheckins') }}</p>
        <p v-if="!entries(k).length" class="note">{{ t('leaderboard.empty') }}</p>
        <ol v-else class="entries">
          <li v-for="(e, i) in entries(k)" :key="i">
            <div class="entry-row">
              <span class="who">{{ e.name }}</span>
              <output>{{ e.value.toLocaleString() }}</output>
            </div>
          </li>
        </ol>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import SelectButton from 'primevue/selectbutton'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { useLiveEvents } from './home'
import { obsPresence, useObsSources } from './obsSources'

type Period = Schemas['Period']
type Kind = Schemas['BoardKind']
const PERIODS: Period[] = ['stream', 'month', 'all']
const KINDS: Kind[] = ['bits', 'gifts', 'chat', 'checkins']

const { t } = useI18n()
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
.period-bar {
  display: flex;
  align-items: center;
}

.boards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(18rem, 100%), 1fr));
  gap: 1rem;
  margin-top: 1rem;
}

.board {
  display: flex;
  flex-direction: column;
  gap: 0.625rem;
  padding: 1rem 1.125rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.board-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
}

.board-head h2 {
  margin: 0;
  font-size: 1rem;
}

.in-obs {
  display: inline-flex;
  align-items: center;
  padding: 0.1875rem 0.625rem;
  border-radius: 999px;
  font-size: 0.75rem;
  font-weight: 500;
  text-decoration: none;
  white-space: nowrap;
}

.in-obs:hover {
  filter: brightness(1.1);
}

.note {
  margin: 0.5rem 0 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.entries {
  display: grid;
  gap: 0.25rem;
  margin: 0;
  padding-inline-start: 1.25rem;
}

.entries li {
  padding: 0.375rem 0;
  border-bottom: 1px solid var(--p-surface-800);
}

.entries li:last-child {
  border-bottom: 0;
}

.entry-row {
  display: flex;
  justify-content: space-between;
  gap: 1rem;
}

.who {
  font-weight: 600;
}

.entries output {
  font-variant-numeric: tabular-nums;
}
</style>
