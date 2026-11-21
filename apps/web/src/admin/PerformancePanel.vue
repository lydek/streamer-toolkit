<template>
  <section class="stack" aria-labelledby="ts-perf">
    <p v-if="perf?.warning" role="alert" class="warning">
      {{ t('performance.warning', { n: perf.obs?.dropped_percent?.toFixed(1) }) }}
    </p>
    <h3 id="ts-perf">{{ t('performance.heading') }}</h3>
    <p v-if="!perf">{{ t('app.loading') }}</p>
    <dl v-else class="facts">
      <dt>{{ t('performance.cpu') }}</dt>
      <dd>{{ percent(perf.cpu_percent) }}</dd>
      <dt>{{ t('performance.memory') }}</dt>
      <dd>{{ t('performance.memoryOf', { used: perf.memory_used_mb, total: perf.memory_total_mb }) }}</dd>
      <dt>{{ t('performance.app') }}</dt>
      <dd>{{ percent(perf.app_cpu_percent) }} · {{ t('performance.mb', { n: perf.app_memory_mb }) }}</dd>
      <dt>{{ t('performance.obsCpu') }}</dt>
      <dd>{{ perf.obs ? percent(perf.obs.cpu_percent) : NONE }}</dd>
      <dt>{{ t('performance.dropped') }}</dt>
      <dd>{{ perf.obs?.dropped_percent != null ? percent(perf.obs.dropped_percent) : NONE }}</dd>
      <dt>{{ t('performance.bitrate') }}</dt>
      <dd>{{ perf.obs?.bitrate_kbps != null ? t('performance.kbps', { n: perf.obs.bitrate_kbps }) : NONE }}</dd>
    </dl>
  </section>
</template>

<script setup lang="ts">
import { onBeforeUnmount, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'

// Shown for OBS's numbers while OBS is not connected (or not streaming).
const NONE = '—'

const { t } = useI18n()
const perf = shallowRef<Schemas['Performance'] | null>()
const percent = (n: number) => `${n.toFixed(1)}%`

async function load() {
  try {
    perf.value = await api(client.GET('/api/admin/performance'))
  } catch {
    // The last numbers stay until the next sample.
  }
}

// The Host samples every 2 s.
const timer = setInterval(() => void load(), 2000)
onBeforeUnmount(() => clearInterval(timer))
void load()
</script>

<style scoped>
.warning {
  padding: 0.5rem 0.75rem;
  border-radius: 6px;
  background: var(--p-surface-800);
}

h3 {
  margin: 0;
}

.facts {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 0.25rem 1rem;
  margin: 0;
}

.facts dd {
  margin: 0;
}
</style>
