<template>
  <FullPage :title="t('settings.title')">
    <div class="settings">
      <!-- One section at a time (route /settings/:section); every section stays mounted, so loading and saving
           work as on one page and switching is a route change (which submits a field still being edited). -->
      <nav class="settings-menu" :aria-label="t('settings.sections')">
        <RouterLink v-for="s in SECTIONS" :key="s" :to="`/settings/${s}`" replace :aria-current="s === section ? 'page' : undefined">
          {{ t(`settings.menu.${s}`) }}
        </RouterLink>
      </nav>
      <div class="settings-body">
        <section v-show="section === 'general'" aria-labelledby="general">
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
          <!-- Dragging changes only the preview; letting go applies it to the window, so the page does not jump. -->
          <div class="text-size">
            <label for="text-size">{{ t('settings.textSize') }}</label>
            <span class="slider">
              <input
                id="text-size"
                type="range"
                :min="TEXT_SIZE.min"
                :max="TEXT_SIZE.max"
                :step="TEXT_SIZE.step"
                :value="textDraft"
                aria-describedby="text-size-preview"
                @input="textDraft = Number(($event.target as HTMLInputElement).value)"
                @change="commitTextSize"
              />
              <output for="text-size">{{ textDraft }}%</output>
            </span>
            <p id="text-size-preview" class="preview" :style="{ fontSize: `${textDraft / textSize}rem` }">
              {{ t('settings.textSizePreview') }}
            </p>
          </div>
        </section>

        <section v-show="section === 'connections'" aria-labelledby="twitch">
          <h2 id="twitch" tabindex="-1">Twitch</h2>
          <TwitchSignIn :overview="overview" :busy="busy" :run="run" />
        </section>

        <section v-show="section === 'connections'" aria-labelledby="obs">
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

        <section v-show="section === 'features'" aria-labelledby="features">
          <h2 id="features" tabindex="-1">{{ t('settings.features') }}</h2>
          <FeatureList :modules="modules" :busy="busy" :load-text="loadText" :with-settings="['checkin']" @toggle="toggleModule">
            <!-- Check-in's own settings, behind 「設定」 on its row. -->
            <template #settings>
              <CheckinSettings @error="report" />
              <SoundSlots
                :slots="[
                  { slot: 'checkin', label: t('sound.checkinNormal') },
                  { slot: 'checkin.full', label: t('sound.checkinFull') },
                ]"
              />
              <CheckinOutbox ref="checkinOutbox" :state="checkinState" @close="(n) => disableDialog?.open(n)" @error="report" />
            </template>
          </FeatureList>
        </section>

        <section v-for="s in SOON" v-show="section === s" :key="s" :aria-labelledby="`soon-${s}`">
          <h2 :id="`soon-${s}`" tabindex="-1">{{ t(`settings.menu.${s}`) }}</h2>
          <p>{{ t(`settings.soon.${s}`) }}</p>
          <p class="soon"><i class="pi pi-hourglass" aria-hidden="true"></i>{{ t('pages.soon') }}</p>
        </section>

        <section v-show="section === 'backup'" aria-labelledby="backup">
          <h2 id="backup" tabindex="-1">{{ t('settings.backup') }}</h2>
          <BackupRestore />
        </section>

        <section v-show="section === 'about'" aria-labelledby="about">
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
      </div>
    </div>
    <CheckinDisableDialog ref="disableDialog" @choose="disableCheckin" />
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute } from 'vue-router'
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
import { TEXT_SIZE, saveTextSize, useTextSize } from './textsize'

// Settings apply at once, like a drawer's (no Save button); leaving the page submits a field still being edited.
const { t } = useI18n()
const toast = useToast()
const route = useRoute()

const SECTIONS = ['general', 'connections', 'features', 'profiles', 'hotkeys', 'debug', 'backup', 'about'] as const
/** Sections of later phases: they say so and call nothing. */
const SOON = ['profiles', 'hotkeys', 'debug'] as const
const section = computed(() => (route.params.section as string | undefined) ?? 'general')
// Switching sections moves focus to the new section's heading, like entering a page; a hash naming a heading in the
// shown section (a status light's #obs) wins, also when only the hash changes.
const HEADINGS: Record<string, string> = { general: 'general', connections: 'twitch', features: 'features', backup: 'backup', about: 'about' }
watch([section, () => route.hash], ([s, hash]) =>
  nextTick(() => {
    const named = hash ? document.getElementById(decodeURIComponent(hash.slice(1))) : null
    // v-show hides the other sections with an inline display: none.
    const shown = named?.closest('section')?.style.display !== 'none' ? named : null
    ;(shown ?? document.getElementById(HEADINGS[s] ?? `soon-${s}`))?.focus()
  }),
)

const overview = ref<Schemas['Overview']>()
const modules = ref<Schemas['ModuleState'][]>()
const loadFailed = ref(false)
const busy = ref(false)
const passwordSet = ref(false)
let timer: number | undefined

const loadText = computed(() => t(loadFailed.value ? 'home.loadFailed' : 'app.loading'))

const textSize = useTextSize()
const textDraft = ref(textSize.value)
watch(textSize, (n) => (textDraft.value = n))
async function commitTextSize() {
  try {
    await saveTextSize(textDraft.value)
  } catch (e) {
    textDraft.value = textSize.value
    report(e)
  }
}

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
.settings {
  display: grid;
  grid-template-columns: 11rem minmax(0, 1fr);
  gap: 1.5rem;
  align-items: start;
  margin-top: 1rem;
}

.settings-menu {
  position: sticky;
  top: calc(var(--appbar-h, 0px) + 1rem);
  display: grid;
  gap: 0.125rem;
}

.settings-menu a {
  padding: 0.5rem 0.75rem;
  border-radius: 6px;
  color: var(--p-text-muted-color);
  text-decoration: none;
}

.settings-menu a:hover {
  color: var(--p-text-color);
}

.settings-menu a[aria-current='page'] {
  background: var(--p-surface-800);
  color: var(--p-text-color);
}

/* A narrow page: the menu becomes one row that scrolls sideways. */
@container view (max-width: 40rem) {
  .settings {
    grid-template-columns: minmax(0, 1fr);
    gap: 1rem;
  }

  .settings-menu {
    position: static;
    display: flex;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .settings-menu a {
    flex: none;
    white-space: nowrap;
  }
}

.text-size {
  display: grid;
  gap: 0.25rem;
  margin-top: 0.75rem;
}

.slider {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  max-width: 20rem;
}

.slider input {
  flex: 1;
  accent-color: var(--p-primary-color);
}

.preview {
  margin: 0;
  color: var(--p-text-muted-color);
}

.soon {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  border: 1px dashed var(--p-surface-500);
  border-radius: 6px;
  color: var(--p-text-muted-color);
}

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
