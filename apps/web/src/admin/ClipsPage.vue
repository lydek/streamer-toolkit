<template>
  <FullPage :title="t('modules.names.clips')">
    <form class="add" @submit.prevent="add">
      <label for="clip-input">{{ t('clips.input') }}</label>
      <div class="row">
        <InputText id="clip-input" v-model="input" :disabled="busy" />
        <Button type="submit" :label="t('clips.add')" :disabled="busy || !input.trim()" />
      </div>
      <p v-if="addError" role="alert">{{ addError }}</p>
    </form>

    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!clips">{{ t('app.loading') }}</p>
    <template v-else>
      <section class="stack" aria-labelledby="clips-now">
        <h2 id="clips-now">{{ t('clips.now') }}</h2>
        <!-- Clip titles and channel names are Twitch text: text interpolation only. -->
        <p data-clip-state>
          {{ t(`clips.state.${clips.state}`) }}
          <template v-if="clips.current">：{{ clips.current.title }}（{{ clips.current.broadcasterName }}）</template>
        </p>
        <p v-if="clips.current?.labels.length">{{ t('clips.labeledHint') }}</p>
        <p v-if="remaining !== null">{{ t('clips.remaining', { s: remaining }) }}</p>
        <div class="row">
          <Button :label="t('clips.skip')" :disabled="!active || !clips.sessionId" @click="command('skip')" />
          <Button :label="t('clips.stop')" outlined :disabled="!active" @click="command('stop')" />
          <Button :label="t('clips.play')" outlined :disabled="!canPlay" @click="command('play')" />
          <Button :label="t('clips.replay')" outlined :disabled="!clips.sessionId" @click="command('replay')" />
        </div>
      </section>

      <section class="stack" aria-labelledby="clips-queue">
        <h2 id="clips-queue">{{ t('clips.queue', { n: clips.queue.length }) }}</h2>
        <p v-if="!clips.queue.length">{{ t('clips.empty') }}</p>
        <template v-else>
          <ol class="queue">
            <li v-for="c in clips.queue" :key="c.entry">
              <span>
                {{ c.title }}（{{ c.broadcasterName }}）{{ t('clips.seconds', { s: seconds(c.durationMs) }) }}
                <template v-if="c.labels.length">｜{{ t('clips.labeledHint') }}</template>
              </span>
              <Button :label="t('clips.remove')" size="small" outlined @click="remove(c.entry)" />
            </li>
          </ol>
          <Button :label="t('clips.clear')" severity="secondary" outlined class="start" @click="clear" />
        </template>
      </section>
    </template>

    <section class="stack" aria-labelledby="clips-overlay">
      <h2 id="clips-overlay">{{ t('clips.overlay') }}</h2>
      <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined class="start" @click="copyObsUrl" />
      <label for="clip-buffer">{{ t('clips.buffer') }}</label>
      <InputNumber
        v-if="settings"
        v-model="settings.bufferSecs"
        input-id="clip-buffer"
        :min="0"
        :max="30"
        show-buttons
        class="start"
        @update:model-value="saveSettings"
      />
      <div v-if="settings" class="row">
        <Checkbox v-model="settings.skipLabeled" input-id="clip-skip-labeled" binary @update:model-value="saveSettings" />
        <label for="clip-skip-labeled">{{ t('clips.skipLabeled') }}</label>
      </div>
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'

const { t } = useI18n()
const toast = useToast()
const clips = shallowRef<Schemas['ClipQueue']>()
const failed = ref('')
const input = ref('')
const addError = ref('')
const busy = ref(false)
const settings = ref<Schemas['ClipSettings']>()
let timer: number | undefined

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))
const report = (e: unknown) => toast.add(errorToast(message(e)))
const seconds = (ms: number) => Math.round(ms / 1000)
const active = computed(() =>
  ['loading', 'estimated_playing', 'transitioning', 'error'].includes(clips.value?.state ?? ''),
)
const canPlay = computed(
  () => (clips.value?.state === 'idle' || clips.value?.state === 'stopped') && clips.value.queue.length > 0,
)
// An estimate from the clip's length; the embed reports no real progress.
const remaining = computed(() => {
  const c = clips.value
  if (c?.state !== 'estimated_playing' || !c.current || c.elapsedMs == null) return null
  return seconds(Math.max(0, c.current.durationMs - c.elapsedMs))
})

async function load() {
  try {
    clips.value = await api(client.GET('/api/admin/clips'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : t('clips.loadFailed')
  }
}

async function add() {
  addError.value = ''
  busy.value = true
  try {
    const added = await api(client.POST('/api/admin/clips', { body: { input: input.value } }))
    toast.add(successToast(t('clips.added', { title: added.title })))
    input.value = ''
    await load()
  } catch (e) {
    addError.value = message(e)
  } finally {
    busy.value = false
  }
}

async function command(name: 'skip' | 'stop' | 'play' | 'replay') {
  // Skip and replay name the session they were meant for, so a double click or a late click does nothing extra.
  const sessionId = clips.value?.sessionId ?? ''
  try {
    if (name === 'skip') await api(client.POST('/api/admin/clips/skip', { body: { sessionId } }))
    else if (name === 'replay') await api(client.POST('/api/admin/clips/replay', { body: { sessionId } }))
    else if (name === 'stop') await api(client.POST('/api/admin/clips/stop'))
    else await api(client.POST('/api/admin/clips/play'))
    await load()
  } catch (e) {
    report(e)
  }
}

async function remove(entry: string) {
  try {
    await api(client.DELETE('/api/admin/clips/{entry}', { params: { path: { entry } } }))
    await load()
  } catch (e) {
    report(e)
  }
}

async function clear() {
  try {
    await api(client.DELETE('/api/admin/clips'))
    await load()
  } catch (e) {
    report(e)
  }
}

async function saveSettings() {
  const body = settings.value
  if (!body || body.bufferSecs == null) return
  try {
    await api(client.PUT('/api/admin/clips/settings', { body: { ...body } }))
  } catch (e) {
    report(e)
  }
}

async function copyObsUrl() {
  try {
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/clip/`)
    toast.add(successToast(t('chat.copied')))
  } catch {
    report(t('chat.copyFailed'))
  }
}

onMounted(async () => {
  // The player moves on by itself, so the state and the remaining time are polled while the page is open.
  // Started before the first await, so leaving during the first load still stops it.
  timer = window.setInterval(() => void load(), 1000)
  await load()
  try {
    settings.value = await api(client.GET('/api/admin/clips/settings'))
  } catch (e) {
    report(e)
  }
})
onBeforeUnmount(() => window.clearInterval(timer))
</script>

<style scoped>
.add,
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.add {
  flex-direction: column;
}

.queue {
  padding-left: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.queue li span {
  margin-right: 0.5rem;
  overflow-wrap: anywhere;
}

.start {
  align-self: flex-start;
}

h2,
p {
  margin: 0;
}
</style>
