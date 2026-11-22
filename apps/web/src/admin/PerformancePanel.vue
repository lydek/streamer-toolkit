<template>
  <section class="stack" aria-labelledby="ts-perf">
    <p v-if="perf?.warning" role="alert" class="warning">
      {{ t('performance.warning', { n: perf.obs?.dropped_percent?.toFixed(1) }) }}
    </p>
    <h3 id="ts-perf">{{ t('performance.heading') }}</h3>
    <p v-if="!perf">{{ t('app.loading') }}</p>
    <dl v-else class="metrics">
      <div>
        <dt>{{ t('performance.cpu') }}</dt>
        <dd>{{ percent(perf.cpu_percent) }}</dd>
      </div>
      <div>
        <dt>{{ t('performance.memory') }}</dt>
        <dd>{{ t('performance.memoryOf', { used: perf.memory_used_mb, total: perf.memory_total_mb }) }}</dd>
      </div>
      <div>
        <dt>{{ t('performance.app') }}</dt>
        <dd>{{ percent(perf.app_cpu_percent) }} · {{ t('performance.mb', { n: perf.app_memory_mb }) }}</dd>
      </div>
      <div>
        <dt>{{ t('performance.obsCpu') }}</dt>
        <dd>{{ perf.obs ? percent(perf.obs.cpu_percent) : NONE }}</dd>
      </div>
      <div>
        <dt>
          {{ t('performance.dropped') }}<small>{{ t('performance.droppedWindow') }}</small>
        </dt>
        <dd>{{ perf.obs?.dropped_percent != null ? percent(perf.obs.dropped_percent) : NONE }}</dd>
      </div>
      <div>
        <dt>{{ t('performance.bitrate') }}</dt>
        <dd>{{ perf.obs?.bitrate_kbps != null ? t('performance.kbps', { n: perf.obs.bitrate_kbps }) : NONE }}</dd>
      </div>
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

/* Each figure with its name; on a phone, two a row. */
.metrics {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(9rem, 40%), 1fr));
  gap: 0.75rem 1rem;
  margin: 0;
}

.metrics dt {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.metrics dd {
  margin: 0;
  font-weight: 600;
}

.metrics small {
  margin-inline-start: 0.25rem;
  font-size: 0.75rem;
}
</style>
