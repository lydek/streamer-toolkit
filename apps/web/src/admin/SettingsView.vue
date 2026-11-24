<template>
  <FullPage :title="t('settings.title')">
    <div class="settings">
      <!-- One section at a time (route /settings/:section); every section stays mounted, so loading and saving
           work as on one page and switching is a route change (which submits a field still being edited). -->
      <nav class="settings-menu" :aria-label="t('settings.sections')">
        <template v-for="s in SECTIONS" :key="s">
          <RouterLink :to="`/settings/${s}`" replace :aria-current="s === section ? 'page' : undefined">
            <i :class="SECTION_ICONS[s]" aria-hidden="true"></i>
            <span>{{ t(`settings.menu.${s}`) }}</span>
          </RouterLink>
          <!-- Under 功能: each group, to jump straight to it, with how many of its features are on. -->
          <template v-if="s === 'features' && modules">
            <a v-for="g in featureGroups" :key="g.id" :href="`#fg-${g.id}`" class="sub" @click.prevent="jumpToGroup(g.id)">
              {{ t(`nav.groups.${g.id}`) }}<span class="sub-count">{{ g.on }}/{{ g.all }}</span>
            </a>
          </template>
        </template>
      </nav>
      <div class="settings-body">
        <section v-show="section === 'general'" aria-labelledby="general">
          <h2 id="general" tabindex="-1">{{ t('settings.general') }}</h2>
          <p v-if="unreadable" role="alert">{{ t('settings.unreadable') }}</p>
          <!-- Dragging changes only the preview; letting go applies it to the window, so the page does not jump. -->
          <FormPanel>
            <FormRow :label="t('settings.textSize')" input-id="text-size">
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
            </FormRow>
          </FormPanel>
          <p v-if="ask.value.value === undefined">{{ loadText }}</p>
          <!-- Each switch shows once its own setting has loaded, so one failing read does not hide the other. -->
          <FormPanel v-if="ask.value.value !== undefined || checkUpdates.value.value !== undefined">
            <FormRow v-if="ask.value.value !== undefined" :label="t('settings.askBeforeClose')" input-id="ask-before-close" end>
              <ToggleSwitch v-model="ask.value.value" input-id="ask-before-close" @update:model-value="ask.commit()" />
            </FormRow>
            <FormRow v-if="checkUpdates.value.value !== undefined" :label="t('update.check')" input-id="check-updates" end>
              <ToggleSwitch v-model="checkUpdates.value.value" input-id="check-updates" @update:model-value="checkUpdates.commit()" />
            </FormRow>
          </FormPanel>
          <FormPanel :heading="t('outputDevice.heading')">
            <OutputDevicePicker />
          </FormPanel>
        </section>

        <section v-show="section === 'connections'" class="card conn-card" aria-labelledby="twitch">
          <div class="card-head">
            <span class="badge-icon tint-primary" aria-hidden="true"><i class="pi pi-twitch"></i></span>
            <div class="card-title">
              <h2 id="twitch" tabindex="-1">Twitch</h2>
              <span v-if="overview" class="sub">
                <span class="dot" :class="twitchDotClass" aria-hidden="true"></span>
                {{ twitchSummary }}
              </span>
            </div>
          </div>
          <div class="card-body">
            <TwitchSignIn :overview="overview" :busy="busy" :run="run" />
            <!-- Not built yet: grey. -->
            <p v-for="s in ['scopes', 'bot']" :key="s" class="off">
              {{ t(`settings.twitch.${s}`) }}<span class="sr-only">{{ t('home.unbuilt') }}</span>
            </p>
          </div>
        </section>

        <section v-show="section === 'connections'" class="card conn-card" aria-labelledby="obs">
          <div class="card-head">
            <span class="badge-icon tint-blue" aria-hidden="true"><i class="pi pi-desktop"></i></span>
            <div class="card-title">
              <h2 id="obs" tabindex="-1">OBS Studio</h2>
              <span id="obs-status" class="sub">
                <span class="dot" :class="obsDotClass" aria-hidden="true"></span>
                {{ obsSummary }}
              </span>
            </div>
            <Button
              v-if="obsHost.value.value !== undefined && !obsEditing"
              id="obs-edit"
              ref="obsEditBtn"
              class="push"
              :label="t('settings.obs.edit')"
              severity="secondary"
              :aria-expanded="obsEditing"
              aria-controls="obs-form"
              @click="openObsEdit"
            />
          </div>
          <p v-if="obsHost.value.value === undefined" class="card-body">{{ loadText }}</p>
          <div v-else v-show="obsEditing" id="obs-form" class="obs-form" @keydown.escape.prevent.stop="closeObsEdit">
            <div class="obs-rows">
              <FormRow :label="t('settings.obs.host')" input-id="obs-host">
                <InputText
                  id="obs-host"
                  v-model="obsHost.value.value"
                  autocomplete="off"
                  @keydown.enter="obsHost.commit()"
                  @blur="obsHost.commit()"
                />
              </FormRow>
              <FormRow :label="t('settings.obs.port')" input-id="obs-port">
                <InputText
                  id="obs-port"
                  v-model="obsPort.value.value"
                  inputmode="numeric"
                  autocomplete="off"
                  @keydown.enter="obsPort.commit()"
                  @blur="obsPort.commit()"
                />
              </FormRow>
              <FormRow :label="t('settings.obs.password')" input-id="obs-password">
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
                <template #hint>
                  <span id="obs-password-state">{{ t(passwordSet ? 'settings.obs.passwordSet' : 'settings.obs.passwordNone') }}</span>
                </template>
              </FormRow>
            </div>
            <div class="card-foot">
              <Button v-if="passwordSet" :label="t('settings.obs.removePassword')" outlined :disabled="busy" @click="removePassword" />
              <Button id="obs-done" class="push" :label="t('settings.obs.done')" @click="closeObsEdit" />
            </div>
          </div>
        </section>

        <section v-show="section === 'features'" aria-labelledby="features">
          <h2 id="features" tabindex="-1">{{ t('settings.features') }}</h2>
          <FeatureList
            :modules="modules"
            :busy="busy"
            :load-text="loadText"
            :with-settings="['checkin']"
            @toggle="toggleModule"
            @bulk="bulkModules"
            @service="switchService"
          >
            <!-- Check-in's held redemptions belong to its switch; its rules have their own page (打卡規則). -->
            <template #settings>
              <CheckinOutbox ref="checkinOutbox" :state="checkinState" @close="(n) => disableDialog?.open(n)" @error="report" />
            </template>
          </FeatureList>
        </section>

        <section v-for="s in SOON" v-show="section === s" :key="s" :aria-labelledby="`soon-${s}`">
          <h2 :id="`soon-${s}`" tabindex="-1">{{ t(`settings.menu.${s}`) }}</h2>
          <p>{{ t(`settings.soon.${s}`) }}</p>
          <p class="soon"><i class="pi pi-hourglass" aria-hidden="true"></i>{{ t('pages.soon') }}</p>
        </section>

        <section v-show="section === 'debug'" aria-labelledby="debug">
          <h2 id="debug" tabindex="-1">{{ t('settings.menu.debug') }}</h2>
          <TroubleshootPanel v-if="section === 'debug'" />
        </section>

        <section v-show="section === 'backup'" aria-labelledby="backup">
          <h2 id="backup" tabindex="-1">{{ t('settings.backup') }}</h2>
          <RecordRetention />
          <BackupRestore />
        </section>

        <section v-show="section === 'about'" aria-labelledby="about">
          <h2 id="about" tabindex="-1">{{ t('settings.about') }}</h2>
          <FormPanel>
            <FormRow :label="t('settings.version')">{{ t('app.name') }} {{ version }}</FormRow>
            <FormRow :label="t('settings.licenseLabel')">{{ t('settings.license') }}</FormRow>
            <details class="more" @toggle="loadLicenses">
              <summary>{{ t('settings.thirdParty') }}</summary>
              <p v-if="licenses === undefined">{{ licensesFailed ? t('home.loadFailed') : t('app.loading') }}</p>
              <!-- Plain text: interpolation escapes it. Focusable so the keyboard can scroll it. -->
              <pre v-else class="licenses" tabindex="0" :aria-label="t('settings.thirdParty')">{{ licenses }}</pre>
            </details>
          </FormPanel>
        </section>
      </div>
    </div>
    <CheckinDisableDialog ref="disableDialog" @choose="disableCheckin" @cancel="pendingBulk = pendingService = undefined" />
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { version } from '../../../desktop/tauri.conf.json'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import BackupRestore from './BackupRestore.vue'
import OutputDevicePicker from './OutputDevicePicker.vue'
import TroubleshootPanel from './TroubleshootPanel.vue'
import RecordRetention from './RecordRetention.vue'
import CheckinDisableDialog from './CheckinDisableDialog.vue'
import CheckinOutbox from './CheckinOutbox.vue'
import { useDrawerSetting } from './drawer'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import FeatureList from './FeatureList.vue'
import { FEATURES, type Service, isOn, shareModules, unusedBy } from './features'
import { saveServices, useServices } from './services'
import { GROUPS } from './tabs'
import { errorToast, useLiveEvents } from './home'
import TwitchSignIn from './TwitchSignIn.vue'
import { TEXT_SIZE, saveTextSize, useTextSize } from './textsize'

// Settings apply at once, like a drawer's (no Save button); leaving the page submits a field still being edited.
const { t } = useI18n()
const toast = useToast()
const route = useRoute()

const SECTIONS = ['general', 'connections', 'features', 'profiles', 'hotkeys', 'debug', 'backup', 'about'] as const
const SECTION_ICONS: Record<(typeof SECTIONS)[number], string> = {
  general: 'pi pi-sliders-h',
  connections: 'pi pi-link',
  features: 'pi pi-th-large',
  profiles: 'pi pi-clone',
  hotkeys: 'pi pi-bolt',
  debug: 'pi pi-wrench',
  backup: 'pi pi-database',
  about: 'pi pi-info-circle',
}
/** Sections of later phases: they say so and call nothing. */
const SOON = ['profiles', 'hotkeys'] as const
const section = computed(() => (route.params.section as string | undefined) ?? 'general')
const router = useRouter()
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
// The connection state follows the live stream; the first read covers the time before its snapshot.
const loadedObsStatus = ref<Schemas['ObsStatusPayload']>()
const live = useLiveEvents()
const obsStatus = computed(() => live.value.obs ?? loadedObsStatus.value)
const obsEditing = ref(false)
const obsEditBtn = ref<{ $el: HTMLElement }>()
let timer: number | undefined

const twitchDotClass = computed(() => {
  const s = overview.value?.status.state
  if (s === 'signed_in') return 'dot-green'
  if (s === 'pending') return 'dot-yellow'
  if (s === 'failed') return 'dot-red'
  return 'dot-muted'
})

const twitchSummary = computed(() => {
  const o = overview.value
  if (!o) return ''
  if (!o.client_id) return t('home.status.twitch.setup')
  if (o.status.state === 'signed_in') return `${t('twitch.signedIn')}${o.status.login}`
  if (o.status.state === 'pending') return t('home.status.twitch.pending')
  if (o.status.state === 'failed') return t('home.status.twitch.failed')
  return t('home.status.twitch.signedOut')
})

const obsDotClass = computed(() => {
  const s = obsStatus.value?.state
  if (s === 'connected') return 'dot-green'
  if (s === 'reconnecting') return 'dot-yellow'
  if (s === 'error') return 'dot-red'
  return 'dot-muted'
})

const obsSummary = computed(() => {
  const host = hostSaved.value
  const port = portSaved.value
  const addr = host && port ? ` · ${host}:${port}` : ''
  const s = obsStatus.value?.state
  if (s === 'connected') return `${t('home.status.connected')}${addr}`
  if (s === 'reconnecting') return `${t('home.status.obs.reconnecting')}${addr}`
  if (s === 'error') return `${t('home.status.obs.error')}${addr}`
  return `OBS Studio${addr}`
})

async function openObsEdit() {
  obsEditing.value = true
  await nextTick()
  document.getElementById('obs-host')?.focus()
}

async function closeObsEdit() {
  // Commit any still-focused field before collapsing; keep open if validation or save fails.
  const ok = await obsHost.commit() && await obsPort.commit() && await commitPassword()
  if (!ok) return
  obsEditing.value = false
  await nextTick()
  obsEditBtn.value?.$el.focus()
}

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
  const ok = await obsPassword.commit()
  if (ok) passwordSaved.value = ''
  return ok
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

const report = (e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))

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
  loadedObsStatus.value = obs.status
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
const disableCheckin = (mode: 'pause' | 'close') => {
  const bulk = pendingBulk.value
  const service = pendingService.value
  pendingBulk.value = pendingService.value = undefined
  if (service) return applyService(service.service, service.off, mode === 'close')
  return bulk ? applyBulk(bulk, mode === 'close') : putModule('checkin', false, mode === 'close')
}

// 使用 Twitch／OBS (FeatureList, already confirmed). Off: the choice and what goes off are saved first, then those
// modules are turned off (check-in asks pause or close first; a cancel changes nothing). On: the modules parked for
// it are turned on again, except those still needing the other service while that is unused.
const services = useServices()
const pendingService = ref<{ service: Service; off: string[] }>()
function switchService(service: Service, use: boolean, off: string[]) {
  if (use) return restoreService(service)
  if (off.includes('checkin') && checkinState.value?.enabled) {
    pendingService.value = { service, off }
    return disableDialog.value?.open()
  }
  return applyService(service, off, false)
}
const applyService = (service: Service, off: string[], closeCheckin: boolean) =>
  run(async () => {
    const before = { unused: [...services.unused], parked: [...services.parked] }
    const done: string[] = []
    try {
      await saveServices({
        unused: [...services.unused.filter((s) => s !== service), service],
        parked: [...services.parked, ...off.map((id) => ({ service, id }))],
      })
      for (const id of off) {
        await api(client.PUT('/api/admin/modules/{id}', { params: { path: { id } }, body: { enabled: false, close: id === 'checkin' && closeCheckin } }))
        done.push(id)
      }
    } catch (e) {
      // All goes back as it was (the modules already off, then the choice), so switching the service off again
      // retries.
      for (const id of done)
        await api(client.PUT('/api/admin/modules/{id}', { params: { path: { id } }, body: { enabled: true, close: false } })).catch(() => {})
      await saveServices(before).catch(() => {})
      await refresh().catch(() => {})
      throw e
    } finally {
      await checkinOutbox.value?.load()
    }
  })
const restoreService = (service: Service) =>
  run(async () => {
    const unused = services.unused.filter((s) => s !== service)
    const stillBlocked = (id: string) => FEATURES.some((f) => f.module === id && unusedBy(f, unused).length)
    const back = services.parked.filter((p) => p.service === service && !stillBlocked(p.id))
    const before = { unused: [...services.unused], parked: [...services.parked] }
    const done: string[] = []
    try {
      // Still needing the other unused service: parked for that one now, back when it is.
      const parked = services.parked.filter((p) => !back.includes(p)).map((p) => (p.service === service ? { ...p, service: unused[0] } : p))
      await saveServices({ unused, parked })
      for (const p of back) {
        await api(client.PUT('/api/admin/modules/{id}', { params: { path: { id: p.id } }, body: { enabled: true, close: false } }))
        done.push(p.id)
      }
    } catch (e) {
      // All goes back as it was (the choice, parked modules, and the ones already on again), so switching the
      // service on again retries.
      for (const id of done)
        await api(client.PUT('/api/admin/modules/{id}', { params: { path: { id } }, body: { enabled: false, close: false } })).catch(() => {})
      await saveServices(before).catch(() => {})
      await refresh().catch(() => {})
      throw e
    } finally {
      await checkinOutbox.value?.load()
    }
  })

// A whole group at once (FeatureList, already confirmed): check-in going off asks pause or close first, and nothing
// is sent if that is cancelled. Applied one by one; a failure shows its reason and the real state is read again.
const pendingBulk = ref<{ id: string; enabled: boolean }[]>()
function bulkModules(changes: { id: string; enabled: boolean }[]) {
  const offCheckin = changes.some((c) => c.id === 'checkin' && !c.enabled) && checkinState.value?.enabled
  if (!offCheckin) return applyBulk(changes, false)
  pendingBulk.value = changes
  disableDialog.value?.open()
}
const applyBulk = (changes: { id: string; enabled: boolean }[], closeCheckin: boolean) =>
  run(async () => {
    try {
      for (const c of changes)
        await api(
          client.PUT('/api/admin/modules/{id}', {
            params: { path: { id: c.id } },
            body: { enabled: c.enabled, close: c.id === 'checkin' && !c.enabled && closeCheckin },
          }),
        )
    } catch (e) {
      await refresh().catch(() => {})
      throw e
    } finally {
      await checkinOutbox.value?.load()
    }
  })

// The settings menu's group links: jump to the group's heading in 功能 (and focus it, so the keyboard is there too).
const featureGroups = computed(() =>
  GROUPS.map((g) => {
    const fs = FEATURES.filter((f) => f.group === g.id && !f.openWith && !f.fixed)
    return { id: g.id, all: fs.length, on: fs.filter((f) => isOn(f, modules.value ?? [])).length }
  }).filter((g) => g.all),
)
async function jumpToGroup(id: string) {
  if (section.value !== 'features') await router.replace('/settings/features')
  await nextTick()
  const heading = document.getElementById(`fg-${id}`)
  heading?.scrollIntoView({ block: 'start' })
  heading?.focus({ preventScroll: true })
}
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

.settings-body {
  container: panel / inline-size;
}

.settings-menu {
  position: sticky;
  top: calc(var(--appbar-h, 0px) + var(--page-top-h, 0px) + 1rem);
  display: grid;
  gap: 0.125rem;
}

.settings-menu a {
  display: flex;
  align-items: center;
  gap: 0.625rem;
  padding: 0.5rem 0.75rem;
  border-radius: 6px;
  color: var(--p-text-muted-color);
  text-decoration: none;
}

.settings-menu a > i {
  width: 1rem;
  text-align: center;
  flex-shrink: 0;
}

.settings-menu a.sub {
  display: flex;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 0.25rem 0.75rem 0.25rem 1.5rem;
  font-size: 0.875rem;
}

.sub-count {
  font-variant-numeric: tabular-nums;
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
  margin: 0.25rem 0 0;
  color: var(--p-text-muted-color);
}

/* Panels of one section, one under another. */
.form-panel + .form-panel,
p + .form-panel {
  margin-top: 1rem;
}

/* The about panel's third-party licenses, under its rows. */
.more {
  padding: 0.625rem 0;
  border-top: 1px solid var(--p-surface-800);
}

.more summary {
  cursor: pointer;
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

.card {
  padding: 1rem 1.25rem;
  border: 1px solid var(--p-content-border-color);
  border-radius: 6px;
}

.conn-card {
  max-width: 52rem;
  padding: 0;
  border: 0;
  background: var(--p-surface-900);
}

.card-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.75rem;
  padding: 1rem 1.25rem;
}

.badge-icon {
  display: grid;
  place-items: center;
  width: 2.25rem;
  height: 2.25rem;
  border-radius: 6px;
  font-size: 1.125rem;
  flex-shrink: 0;
}

.card-title {
  display: flex;
  flex-direction: column;
  gap: 0.125rem;
  min-width: 0;
}

.card-head h2 {
  margin: 0;
  font-size: 1rem;
}

.sub {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  flex-shrink: 0;
}

.dot-green {
  background: var(--p-green-400);
}

.dot-yellow {
  background: var(--p-yellow-400);
}

.dot-red {
  background: var(--p-red-400);
}

.dot-muted {
  background: var(--p-surface-500);
}

.push {
  margin-left: auto;
}

.card-body {
  padding: 0 1.25rem 1rem;
}

.obs-form {
  border-top: 1px solid var(--p-surface-800);
}

.obs-rows {
  container-type: inline-size;
  padding: 0 1.25rem;
}

.card-foot {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  padding: 0.75rem 1.25rem;
  border-top: 1px solid var(--p-surface-800);
}

.card + .card {
  margin-top: 1rem;
}

.card h2 {
  margin-top: 0;
}

.off {
  color: var(--p-text-muted-color);
}
</style>
