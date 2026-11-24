<template>
  <FullPage tile="rehearsal" :status="running ? t('rehearsal.running') : undefined">
    <div class="field">
      <span id="rh-speed">{{ t('rehearsal.speed') }}</span>
      <SelectButton
        v-model="speed"
        :options="SPEEDS"
        :option-label="(s: Speed) => t(`rehearsal.speeds.${s}`)"
        :allow-empty="false"
        aria-labelledby="rh-speed"
      />
    </div>

    <section class="stack" aria-labelledby="rh-built-in">
      <h2 id="rh-built-in">{{ t('rehearsal.builtIn') }}</h2>
      <ul class="plain scripts">
        <li v-for="id in BUILT_IN" :key="id">
          <span>{{ t(`rehearsal.scripts.${id}`) }}</span>
          <Button :label="t('rehearsal.start')" icon="pi pi-play" size="small" @click="start(() => runBuiltIn(id, speed))" />
        </li>
      </ul>
    </section>

    <section class="stack" aria-labelledby="rh-own">
      <h2 id="rh-own">{{ t('rehearsal.own') }}</h2>
      <label for="rh-file">{{ t('rehearsal.file') }}</label>
      <input id="rh-file" type="file" accept=".json,application/json" @change="importFile" />
    </section>

    <p v-if="failed" role="alert">{{ failed }}</p>
    <Button :label="t('rehearsal.stop')" icon="pi pi-stop" severity="danger" outlined :disabled="!running" @click="stop" />
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import SelectButton from 'primevue/selectbutton'
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import FullPage from './FullPage.vue'
import { BUILT_IN, type Speed, runBuiltIn, runScript, stopScript } from './rehearsal'

const SPEEDS: Speed[] = ['normal', 'double', 'burst']

const { t } = useI18n()
const speed = ref<Speed>('normal')
const running = ref(false)
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

async function start(run: () => ReturnType<typeof runScript>) {
  failed.value = ''
  try {
    const { duration_ms } = await run()
    running.value = true
    clearTimeout(done)
    done = setTimeout(() => (running.value = false), duration_ms + 500)
  } catch (e) {
    failed.value = message(e)
  }
}

async function importFile(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (file) {
    const script = await file.text()
    await start(() => runScript(script, speed.value))
  }
}

async function stop() {
  clearTimeout(done)
  running.value = false
  try {
    await stopScript()
  } catch {
    // Already finished: nothing to stop.
  }
}
</script>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.25rem;
}

.scripts li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  max-width: 32rem;
  padding: 0.375rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}
</style>
