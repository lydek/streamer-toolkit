<template>
  <!-- 最近事件 on the alerts page: what became of each alert this launch, and 重播 with the current settings. -->
  <section aria-labelledby="recent-alerts">
    <h2 id="recent-alerts">{{ t('alerts.recent.title') }}</h2>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="recent && !recent.length" class="muted">{{ t('alerts.recent.empty') }}</p>
    <ul v-else-if="recent" class="plain recent">
      <li v-for="r in recent" :key="r.id">
        <time :datetime="r.occurredAt">{{ clock(r.occurredAt) }}</time>
        <span class="what">
          {{ alertTitle(r.payload) }}
          <span v-if="r.source === 'simulation'" class="tag">{{ t('alerts.recent.simulated') }}</span>
        </span>
        <span class="outcome" :class="{ bad: isBad(r.outcome) }">{{ t(`alerts.recent.outcome.${r.outcome ?? 'pending'}`) }}</span>
        <Button :label="t('alerts.recent.replay')" text size="small" :disabled="busy === r.id" @click="replay(r.id)" />
      </li>
    </ul>
  </section>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { alertTitle } from '../overlay/alerts/alerts'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast, successToast } from './home'

// Outcomes change as the queue plays; read the list again this often while the page is open.
const POLL_MS = 2000

const { t } = useI18n()
const toast = useToast()
const recent = shallowRef<Schemas['RecentAlertBody'][]>()
const failed = ref('')
const busy = ref<string>()

async function load() {
  try {
    recent.value = await api(client.GET('/api/admin/alerts/recent'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}
void load()
const timer = window.setInterval(() => void load(), POLL_MS)
onBeforeUnmount(() => window.clearInterval(timer))

const two = (n: number) => String(n).padStart(2, '0')
const clock = (at: string) => {
  const d = new Date(at)
  return `${two(d.getHours())}:${two(d.getMinutes())}`
}
const isBad = (o: Schemas['Outcome'] | null) => o !== null && o !== 'played'

async function replay(id: string) {
  busy.value = id
  try {
    await api(client.POST('/api/admin/alerts/recent/{id}/replay', { params: { path: { id } } }))
    toast.add(successToast(t('alerts.recent.replayed')))
    await load()
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = undefined
  }
}
</script>

<style scoped>
.muted,
time {
  color: var(--p-text-muted-color);
}

.recent li {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.25rem 0.75rem;
  padding: 0.25rem 0;
}

time {
  font-variant-numeric: tabular-nums;
}

.what {
  flex: 1 1 12rem;
  overflow-wrap: anywhere;
}

.tag {
  margin-left: 0.25rem;
  padding: 0 0.375rem;
  border-radius: 999px;
  background: var(--p-surface-800);
  color: var(--p-text-muted-color);
  font-size: 0.75rem;
}

.outcome {
  font-size: 0.875rem;
}

.outcome.bad {
  color: var(--p-yellow-400);
}
</style>
