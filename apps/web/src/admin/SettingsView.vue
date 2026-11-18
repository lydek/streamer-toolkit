<template>
  <FullPage :title="t('settings.title')">
    <section aria-labelledby="general">
      <h2 id="general" tabindex="-1">{{ t('settings.general') }}</h2>
      <p v-if="unreadable" role="alert">{{ t('settings.unreadable') }}</p>
      <p v-if="ask.value.value === undefined">{{ loadText }}</p>
      <div v-else class="choice">
        <Checkbox v-model="ask.value.value" input-id="ask-before-close" binary @update:model-value="ask.commit()" />
        <label for="ask-before-close">{{ t('settings.askBeforeClose') }}</label>
      </div>
      <div v-if="checkUpdates.value.value !== undefined" class="choice">
        <Checkbox v-model="checkUpdates.value.value" input-id="check-updates" binary @update:model-value="checkUpdates.commit()" />
        <label for="check-updates">{{ t('update.check') }}</label>
      </div>
    </section>

    <section aria-labelledby="twitch">
      <h2 id="twitch" tabindex="-1">Twitch</h2>
      <TwitchSignIn :overview="overview" :busy="busy" :run="run" />
    </section>

    <section aria-labelledby="obs">
      <h2 id="obs" tabindex="-1">OBS</h2>
      <p v-if="obsHost.value.value === undefined">{{ loadText }}</p>
      <div v-else class="stack">
        <label for="obs-host">{{ t('settings.obs.host') }}</label>
        <InputText
          id="obs-host"
          v-model="obsHost.value.value"
          autocomplete="off"
          @keydown.enter="obsHost.commit()"
          @blur="obsHost.commit()"
        />
        <label for="obs-port">{{ t('settings.obs.port') }}</label>
        <InputText
          id="obs-port"
          v-model="obsPort.value.value"
          inputmode="numeric"
          autocomplete="off"
          @keydown.enter="obsPort.commit()"
          @blur="obsPort.commit()"
        />
        <label for="obs-password">{{ t('settings.obs.password') }}</label>
        <!-- Write-only: the Host never returns the password, only whether one is stored. -->
        <InputText
          id="obs-password"
          v-model="obsPassword.value.value"
          type="password"
          autocomplete="new-password"
          aria-describedby="obs-password-state"
          @keydown.enter="commitPassword"
          @blur="commitPassword"
        />
        <p id="obs-password-state">{{ t(passwordSet ? 'settings.obs.passwordSet' : 'settings.obs.passwordNone') }}</p>
        <Button v-if="passwordSet" :label="t('settings.obs.removePassword')" outlined :disabled="busy" @click="removePassword" />
      </div>
    </section>

    <section aria-labelledby="features">
      <h2 id="features" tabindex="-1">{{ t('settings.features') }}</h2>
      <FeatureList :modules="modules" :busy="busy" :load-text="loadText" @toggle="toggleModule" />
    </section>

    <section aria-labelledby="checkin">
      <h2 id="checkin" tabindex="-1">{{ t('modules.names.checkin') }}</h2>
      <CheckinSettings @error="report" />
      <SoundSlots
        :slots="[
          { slot: 'checkin', label: t('sound.checkinNormal') },
          { slot: 'checkin.full', label: t('sound.checkinFull') },
        ]"
      />
      <CheckinOutbox ref="checkinOutbox" :state="checkinState" @close="(n) => disableDialog?.open(n)" @error="report" />
    </section>
    <CheckinDisableDialog ref="disableDialog" @choose="disableCheckin" />

    <section aria-labelledby="backup">
      <h2 id="backup" tabindex="-1">{{ t('settings.backup') }}</h2>
      <BackupRestore />
    </section>

    <section aria-labelledby="about">
      <h2 id="about" tabindex="-1">{{ t('settings.about') }}</h2>
      <p>{{ t('app.name') }} {{ version }}</p>
      <p>{{ t('settings.license') }}</p>
      <details @toggle="loadLicenses">
        <summary>{{ t('settings.thirdParty') }}</summary>
        <p v-if="licenses === undefined">{{ licensesFailed ? t('home.loadFailed') : t('app.loading') }}</p>
        <!-- Plain text: interpolation escapes it. Focusable so the keyboard can scroll it. -->
        <pre v-else class="licenses" tabindex="0" :aria-label="t('settings.thirdParty')">{{ licenses }}</pre>
      </details>
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { version } from '../../../desktop/tauri.conf.json'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import BackupRestore from './BackupRestore.vue'
import CheckinDisableDialog from './CheckinDisableDialog.vue'
import CheckinOutbox from './CheckinOutbox.vue'
import CheckinSettings from './CheckinSettings.vue'
import SoundSlots from './SoundSlots.vue'
import { useDrawerSetting } from './drawer'
import FullPage from './FullPage.vue'
import FeatureList from './FeatureList.vue'
import { shareModules } from './features'
import { errorToast } from './home'
import TwitchSignIn from './TwitchSignIn.vue'

// Settings apply at once, like a drawer's (no Save button); leaving the page submits a field still being edited.
const { t } = useI18n()
const toast = useToast()

const overview = ref<Schemas['Overview']>()
const modules = ref<Schemas['ModuleState'][]>()
const loadFailed = ref(false)
const busy = ref(false)
const passwordSet = ref(false)
let timer: number | undefined

const loadText = computed(() => t(loadFailed.value ? 'home.loadFailed' : 'app.loading'))

// Saved values are undefined until loaded; their fields show only after that.
const askSaved = ref<boolean>()
const unreadable = ref(false)
const ask = useDrawerSetting(askSaved, (v) => api(client.PUT('/api/admin/general', { body: { ask_before_close: v! } })))

const checkSaved = ref<boolean>()
const checkUpdates = useDrawerSetting(checkSaved, (v) =>
  api(client.PUT('/api/admin/update/check', { body: { enabled: v! } })),
)

// PUT /api/admin/obs takes the whole address, so saves run one at a time from the last saved one: two fields
// committed in a row must not undo each other.
const savedAddress = { host: '', port: 0 }
let obsQueue: Promise<unknown> = Promise.resolve()
function putObs(patch: { host?: string; port?: number; password?: string }) {
  const run = obsQueue.then(async () => {
    const body = { ...savedAddress, ...patch }
    await api(client.PUT('/api/admin/obs', { body }))
    Object.assign(savedAddress, { host: body.host, port: body.port })
  })
  obsQueue = run.catch(() => {})
  return run
}
const hostSaved = ref<string>()
const obsHost = useDrawerSetting(hostSaved, (host) => putObs({ host }))
// Kept as text, the way the field holds it; the Host validates the host name.
const portSaved = ref<string>()
const obsPort = useDrawerSetting(portSaved, (text) => {
  const port = Number(text)
  return Number.isInteger(port) && port >= 1 && port <= 65535
    ? putObs({ port })
    : Promise.reject(new ApiError('obs.invalid_address'))
})
// Always '' once saved, so the field empties again and an untouched or emptied field sends nothing.
const passwordSaved = ref('')
const obsPassword = useDrawerSetting(passwordSaved, async (password) => {
  await putObs({ password })
  passwordSet.value = true
})

async function commitPassword() {
  if (await obsPassword.commit()) passwordSaved.value = ''
}

// Generated at release build time (scripts/third-party-licenses.mjs) into the frontend's public folder, so dev
// builds have none and show the load failure.
const licenses = ref<string>()
const licensesFailed = ref(false)
async function loadLicenses(e: Event) {
  if (!(e.target as HTMLDetailsElement).open || licenses.value !== undefined) return
  licensesFailed.value = false
  try {
    const res = await fetch('/THIRD_PARTY_LICENSES.txt')
    if (!res.ok || !res.headers.get('content-type')?.startsWith('text/plain')) throw new Error(String(res.status))
    licenses.value = await res.text()
  } catch {
    licensesFailed.value = true
  }
}

const report = (e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))

async function run(action: () => Promise<unknown>) {
  busy.value = true
  try {
    await action()
    await refresh()
  } catch (e) {
    report(e)
  } finally {
    busy.value = false
  }
}

async function refreshTwitch() {
  overview.value = await api(client.GET('/api/admin/twitch'))
}

async function refreshModules() {
  modules.value = await api(client.GET('/api/admin/modules'))
  shareModules(modules.value)
}

async function refresh() {
  await Promise.all([refreshTwitch(), refreshModules()])
}

async function loadGeneral() {
  const before = askSaved.value
  const { ask_before_close, settings_unreadable } = await api(client.GET('/api/admin/general'))
  unreadable.value = settings_unreadable ?? false
  // A toggle saved while this read was in flight is newer than what it returned.
  if (askSaved.value === before) askSaved.value = ask_before_close
}

async function loadUpdateCheck() {
  checkSaved.value = (await api(client.GET('/api/admin/update'))).check
}

async function loadObs() {
  const obs = await api(client.GET('/api/admin/obs'))
  Object.assign(savedAddress, { host: obs.host, port: obs.port })
  hostSaved.value = obs.host
  portSaved.value = String(obs.port)
  passwordSet.value = obs.password_set
}

// Each group loads on its own, so one failing endpoint (e.g. the Credential Manager behind OBS) does not hide
// the others, above all the module switches an emptied home sends the user to. Twitch is polled throughout.
async function load() {
  const results = await Promise.allSettled([
    askSaved.value === undefined && loadGeneral(),
    checkSaved.value === undefined && loadUpdateCheck(),
    hostSaved.value === undefined && loadObs(),
    modules.value === undefined && refreshModules(),
    refreshTwitch(),
  ])
  const failed = results.find((r) => r.status === 'rejected')
  if (failed) {
    loadFailed.value = true
    throw failed.reason
  }
}

// The close hint's "Don't show again" can change this while the page is open; the window is hidden then, so
// reading it again when the window is shown or focused again is enough.
const reloadGeneral = () => {
  if (document.visibilityState === 'visible') void loadGeneral().catch(() => {})
}

const disableDialog = ref<InstanceType<typeof CheckinDisableDialog>>()
const checkinOutbox = ref<InstanceType<typeof CheckinOutbox>>()
const checkinState = computed(() => modules.value?.find((m) => m.id === 'checkin'))

const putModule = (id: string, enabled: boolean, close = false) =>
  run(async () => {
    await api(client.PUT('/api/admin/modules/{id}', { params: { path: { id } }, body: { enabled, close } }))
    await checkinOutbox.value?.load()
  })
// Check-ins take redemptions through the outbox, so turning them off asks whether to keep (pause) or skip them.
const toggleModule = (m: Schemas['ModuleState']) =>
  m.id === 'checkin' && m.enabled ? disableDialog.value?.open() : putModule(m.id, !m.enabled)
const disableCheckin = (mode: 'pause' | 'close') => putModule('checkin', false, mode === 'close')
const removePassword = () =>
  run(async () => {
    await putObs({ password: '' })
    passwordSet.value = false
  })

onMounted(async () => {
  window.addEventListener('focus', reloadGeneral)
  document.addEventListener('visibilitychange', reloadGeneral)
  // Polled so a sign-in finished in the browser shows up, and so groups that failed to load retry. Started before
  // the first await: leaving the page before the first load ends must still stop it.
  timer = window.setInterval(() => void load().catch(() => {}), 2000)
  await load().catch(report)
})
onBeforeUnmount(() => {
  window.clearInterval(timer)
  window.removeEventListener('focus', reloadGeneral)
  document.removeEventListener('visibilitychange', reloadGeneral)
})
</script>

<style scoped>
.licenses {
  max-height: 60vh;
  overflow: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.plain {
  list-style: none;
  padding: 0;
}

.choice {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
</style>
