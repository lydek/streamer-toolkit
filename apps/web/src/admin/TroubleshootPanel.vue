<template>
  <div class="stack">
    <section class="stack" aria-labelledby="ts-connections">
      <div class="head">
        <h3 id="ts-connections">{{ t('troubleshoot.connections') }}</h3>
        <Button :label="t('troubleshoot.check')" icon="pi pi-refresh" text :disabled="checking" @click="check" />
      </div>
      <p v-if="!conn">{{ t('app.loading') }}</p>
      <dl v-else class="facts">
        <dt>{{ t('troubleshoot.twitchApi') }}</dt>
        <dd>
          {{
            !conn.twitch_signed_in
              ? t('troubleshoot.signedOut')
              : conn.twitch_latency_ms == null
                ? t('troubleshoot.failed')
                : t('troubleshoot.latency', { ms: conn.twitch_latency_ms })
          }}
        </dd>
        <dt>EventSub</dt>
        <dd>
          {{ t(conn.eventsub_connected ? 'troubleshoot.connected' : 'troubleshoot.disconnected') }}
          <template v-if="conn.eventsub_subscriptions != null">
            · {{ t('troubleshoot.subscriptions', { n: conn.eventsub_subscriptions }) }}
          </template>
        </dd>
        <dt>OBS</dt>
        <dd>{{ t(conn.obs_connected ? 'troubleshoot.connected' : 'troubleshoot.disconnected') }}</dd>
      </dl>
    </section>

    <section class="stack" aria-labelledby="ts-log">
      <div class="head">
        <h3 id="ts-log">{{ t('troubleshoot.log') }}</h3>
        <Select
          v-model="level"
          :options="LEVELS"
          :option-label="(l: string) => t(`troubleshoot.levels.${l}`)"
          :aria-label="t('troubleshoot.level')"
          @update:model-value="loadLog"
        />
      </div>
      <p v-if="!lines">{{ t('app.loading') }}</p>
      <p v-else-if="!lines.length">{{ t('troubleshoot.noLines') }}</p>
      <ol v-else class="plain log">
        <li v-for="(l, i) in lines" :key="i" :data-level="l.level">
          <time :datetime="l.at">{{ formatTime(l.at) }}</time>
          <span class="level">{{ l.level }}</span>
          <code v-if="l.code">{{ l.code }}</code>
          <span>{{ l.message }}</span>
        </li>
      </ol>
      <div class="actions">
        <Button :label="t('troubleshoot.openFolder')" icon="pi pi-folder-open" outlined @click="openFolder" />
        <Button :label="t('troubleshoot.export')" icon="pi pi-download" outlined @click="exportReport" />
      </div>
      <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
    </section>
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import Button from 'primevue/button'
import Select from 'primevue/select'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { type Schemas, api, client } from './api'
import { saveFile } from './save'

type Level = 'error' | 'warn' | 'info'
const LEVELS: Level[] = ['error', 'warn', 'info']

const { t, locale } = useI18n()
const conn = shallowRef<Schemas['Connections']>()
const checking = ref(false)
const level = ref<Level>('warn')
const lines = shallowRef<Schemas['LogLine'][]>()
const notice = ref<{ text: string; error: boolean }>()
const formatTime = (iso: string) => new Date(iso).toLocaleString(locale.value, { dateStyle: 'short', timeStyle: 'medium' })

async function check() {
  checking.value = true
  try {
    conn.value = await api(client.GET('/api/admin/diagnostics/connections'))
  } catch {
    // The checks stay as they were; the next one tries again.
  } finally {
    checking.value = false
  }
}

async function loadLog() {
  try {
    lines.value = await api(client.GET('/api/admin/diagnostics/logs', { params: { query: { level: level.value } } }))
  } catch {
    lines.value = []
  }
}

async function openFolder() {
  notice.value = undefined
  try {
    await invoke('open_logs_folder')
  } catch (e) {
    notice.value = { text: errorMessage(String(e)), error: true }
  }
}

async function exportReport() {
  notice.value = undefined
  const d = new Date()
  const stamp = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}`
  try {
    const saved = await saveFile(
      `streamer-toolkit-diagnostics-${stamp}.json`,
      { description: 'JSON', mime: 'application/json', extension: '.json' },
      async () => {
        const report = await api(client.GET('/api/admin/diagnostics/report'))
        return new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' })
      },
    )
    if (saved) notice.value = { text: t('troubleshoot.exported'), error: false }
  } catch (e) {
    notice.value = { text: String(e), error: true }
  }
}

void check()
void loadLog()
</script>

<style scoped>
.head {
  display: flex;
  align-items: center;
  gap: 1rem;
}

.head h3 {
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

.log {
  max-height: 20rem;
  overflow-y: auto;
  font-size: 0.875rem;
}

.log li {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem 0.75rem;
  padding: 0.25rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}

.log li[data-level='ERROR'] .level {
  color: var(--p-red-400);
}

.log li[data-level='WARN'] .level {
  color: var(--p-yellow-400);
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
</style>
