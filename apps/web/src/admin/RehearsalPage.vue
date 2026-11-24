<template>
  <FullPage tile="rehearsal" :status="running ? t('rehearsal.running') : t('rehearsal.summary')">
    <div class="banner tint-green">
      <i class="pi pi-shield" aria-hidden="true"></i>
      <span>{{ t('rehearsal.banner') }}</span>
    </div>

    <div v-if="running" class="rh-running tint-red" role="status">
      <span class="dot" aria-hidden="true"></span>
      <span>{{ runningName ? `${t('rehearsal.running')}：${runningName}` : t('rehearsal.running') }}</span>
      <Button class="push" :label="t('rehearsal.stop')" icon="pi pi-stop" severity="danger" @click="stop" />
    </div>

    <section class="section" aria-labelledby="rh-built-in">
      <div class="sec-head">
        <h2 id="rh-built-in">{{ t('rehearsal.builtIn') }}</h2>
        <span id="rh-speed" class="sr-only">{{ t('rehearsal.speed') }}</span>
        <div class="push">
          <SelectButton
            v-model="speed"
            :options="SPEEDS"
            :option-label="(s: Speed) => t(`rehearsal.speeds.${s}`)"
            :allow-empty="false"
            aria-labelledby="rh-speed"
          />
        </div>
      </div>

      <ul class="mode-grid">
        <li v-for="id in BUILT_IN" :key="id" class="mode-card">
          <div class="mode-top">
            <i :class="ICONS[id]" aria-hidden="true"></i>
            <b>{{ t(`rehearsal.scripts.${id}`) }}</b>
          </div>
          <p class="desc">{{ t(`rehearsal.descriptions.${id}`) }}</p>
          <div class="mode-act">
            <Button
              :label="t('rehearsal.start')"
              icon="pi pi-play"
              @click="start(() => runBuiltIn(id, speed), t(`rehearsal.scripts.${id}`))"
            />
          </div>
        </li>
      </ul>
    </section>

    <section class="own-card" aria-labelledby="rh-own">
      <h2 id="rh-own">{{ t('rehearsal.own') }}</h2>
      <div class="push">
        <FilePick
          input-id="rh-file"
          :label="t('rehearsal.file')"
          :hint="t('rehearsal.fileHint')"
          icon="pi pi-upload"
          accept=".json,application/json"
          @pick="importFile"
        />
      </div>
    </section>

    <p v-if="failed" role="alert" class="error">{{ failed }}</p>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import SelectButton from 'primevue/selectbutton'
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import FilePick from './FilePick.vue'
import FullPage from './FullPage.vue'
import { BUILT_IN, type Speed, runBuiltIn, runScript, stopScript } from './rehearsal'

const SPEEDS: Speed[] = ['normal', 'double', 'burst']
const ICONS: Record<(typeof BUILT_IN)[number], string> = {
  checkins: 'pi pi-users',
  raid: 'pi pi-send',
  opening: 'pi pi-bolt',
  stop: 'pi pi-stop-circle',
}

const { t } = useI18n()
const speed = ref<Speed>('normal')
const running = ref(false)
const runningName = ref('')
const failed = ref('')
let done: ReturnType<typeof setTimeout> | undefined
onBeforeUnmount(() => clearTimeout(done))

function message(e: unknown) {
  if (!(e instanceof ApiError)) return String(e)
  const { line, step } = e.params as { line?: number | null; step?: number | null }
  if (line) return t('rehearsal.badLine', { line })
  if (step) return t('rehearsal.badStep', { step })
  return errorMessage(e.code)
}

async function start(run: () => ReturnType<typeof runScript>, label: string) {
  failed.value = ''
  try {
    const { duration_ms } = await run()
    running.value = true
    runningName.value = label
    clearTimeout(done)
    done = setTimeout(() => {
      running.value = false
      runningName.value = ''
    }, duration_ms + 500)
  } catch (e) {
    failed.value = message(e)
  }
}

async function importFile(files: File[]) {
  const file = files[0]
  if (file) {
    const script = await file.text()
    await start(() => runScript(script, speed.value), file.name)
  }
}

async function stop() {
  clearTimeout(done)
  running.value = false
  runningName.value = ''
  try {
    await stopScript()
  } catch {
    // Already finished: nothing to stop.
  }
}
</script>

<style scoped>
.banner {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.75rem 1rem;
  border-radius: 6px;
  margin-bottom: 1rem;
}

.banner span {
  color: var(--p-text-color);
  font-size: 0.875rem;
}

.rh-running {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.75rem;
  padding: 0.625rem 0.875rem;
  border-radius: 6px;
  margin-bottom: 1rem;
  color: var(--p-text-color);
}

.dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  background: var(--p-red-400);
  flex-shrink: 0;
}

.section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.sec-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.sec-head h2,
.own-card h2 {
  margin: 0;
}

.push {
  margin-left: auto;
}

.mode-grid {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(18rem, 100%), 1fr));
  gap: 0.875rem;
}

.mode-card {
  display: flex;
  flex-direction: column;
  gap: 0.625rem;
  padding: 1rem 1.125rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.mode-top {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 1rem;
}

.mode-top i {
  color: var(--p-primary-400);
}

.desc {
  margin: 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
  flex: 1;
}

.mode-act {
  display: flex;
  padding-top: 0.25rem;
}

.own-card {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.75rem 1rem;
  max-width: 52rem;
  margin-top: 1.25rem;
  padding: 1rem 1.25rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.error {
  margin-top: 0.75rem;
  color: var(--p-red-400);
}
</style>
