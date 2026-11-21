<template>
  <!-- The presentation queue on home: what plays (with 跳過), how many wait and about how long, and 清空排隊. -->
  <section class="queue" aria-labelledby="queue-heading">
    <div class="queue-head">
      <h2 id="queue-heading">{{ t('queue.title') }}</h2>
      <span class="summary">{{ summary }}</span>
      <Button :label="t('queue.clear')" text size="small" :disabled="busy" @click="clear" />
      <slot name="actions" />
    </div>
    <ol v-if="items.length" class="queue-list">
      <li v-for="(item, i) in items" :key="i" :class="{ playing: item.playing }">
        <span class="tag">{{ item.playing ? t('queue.playing') : i + 1 }}</span>
        <span>{{ subjectText(item.subject) }}</span>
        <span v-if="item.problem" class="problem">{{ t(`queue.problem.${item.problem}`) }}</span>
        <span v-if="!item.onAir" class="problem">{{ t('queue.offAir') }}</span>
        <Button v-if="item.playing" :label="t('queue.skip')" text size="small" class="skip" :disabled="busy" @click="skip" />
      </li>
    </ol>
  </section>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { computed, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast, successToast } from './home'
import { subjectText, waitMs } from './queue'

const props = defineProps<{ status?: Schemas['QueueStatus'] }>()
const { t } = useI18n()
const toast = useToast()
const busy = ref(false)

// The wait counts down between status events.
const now = ref(Date.now())
const tick = window.setInterval(() => (now.value = Date.now()), 1000)
onBeforeUnmount(() => window.clearInterval(tick))

const items = computed(() => [
  ...(props.status?.current ? [{ subject: props.status.current.subject, problem: props.status.current.problem, onAir: true, playing: true }] : []),
  ...(props.status?.waiting ?? []).map((w) => ({ subject: w.subject, problem: null, onAir: w.onAir, playing: false })),
])


async function skip() {
  busy.value = true
  try {
    const { skipped } = await api(client.POST('/api/admin/queue/skip'))
    if (!skipped) toast.add({ severity: 'info', summary: t('queue.nothingPlaying'), life: 3000 })
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = false
  }
}
const summary = computed(() => {
  const waiting = props.status?.waiting.length ?? 0
  const seconds = Math.ceil(waitMs(props.status, now.value) / 1000)
  return seconds > 0 ? t('queue.summary', { waiting, seconds }) : t('queue.summaryIdle', { waiting })
})

async function clear() {
  busy.value = true
  try {
    const { cancelled } = await api(client.POST('/api/admin/queue/clear'))
    toast.add(
      cancelled
        ? successToast(t('queue.cleared', { cancelled }))
        : { severity: 'info', summary: t('queue.nothingWaiting'), life: 3000 },
    )
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.queue-head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 0.25rem 0.75rem;
}

.queue-head h2 {
  margin: 0;
}

.summary {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.queue-head :deep(.p-button),
.skip {
  margin-left: auto;
}

.problem {
  color: var(--p-yellow-400);
  font-size: 0.875rem;
}

.queue-list {
  display: grid;
  gap: 0.25rem;
  margin: 0.5rem 0 0;
  padding: 0;
  list-style: none;
}

.queue-list li {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  overflow-wrap: anywhere;
}

.tag {
  flex: none;
  min-width: 1.5rem;
  padding: 0 0.375rem;
  border-radius: 999px;
  background: var(--p-surface-800);
  color: var(--p-text-muted-color);
  font-size: 0.75rem;
  text-align: center;
}

.playing .tag {
  background: var(--p-primary-color);
  color: var(--p-primary-contrast-color);
}
</style>
