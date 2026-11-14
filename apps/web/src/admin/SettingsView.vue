<template>
  <FullPage :title="t('settings.title')">
    <section aria-labelledby="general">
      <h2 id="general" tabindex="-1">{{ t('settings.general') }}</h2>
      <p v-if="ask.value.value === undefined">{{ loadText }}</p>
      <div v-else class="choice">
        <Checkbox v-model="ask.value.value" input-id="ask-before-close" binary @update:model-value="ask.commit()" />
        <label for="ask-before-close">{{ t('settings.askBeforeClose') }}</label>
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
      <p v-if="!modules">{{ loadText }}</p>
      <ul v-else class="plain">
        <li v-for="m in modules" :key="m.id" class="choice">
          <Checkbox :input-id="`module-${m.id}`" :model-value="m.enabled" binary :disabled="busy" @update:model-value="toggleModule(m)" />
          <label :for="`module-${m.id}`">{{ moduleName(m.id) }}</label>
        </li>
      </ul>
    </section>

    <!-- Slice 6 adds backup and restore here. -->
    <section aria-labelledby="backup">
      <h2 id="backup" tabindex="-1">{{ t('settings.backup') }}</h2>
      <p>{{ t('settings.backupLater') }}</p>
    </section>

    <section aria-labelledby="about">
      <h2 id="about" tabindex="-1">{{ t('settings.about') }}</h2>
      <p>{{ t('app.name') }} {{ version }}</p>
      <p>{{ t('settings.license') }}</p>
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
import { useDrawerSetting } from './drawer'
import FullPage from './FullPage.vue'
import { errorToast } from './home'
import TwitchSignIn from './TwitchSignIn.vue'

// Settings apply at once, like a drawer's (no Save button); leaving the page submits a field still being edited.
const { t, te } = useI18n()
const toast = useToast()

const overview = ref<Schemas['Overview']>()
const modules = ref<Schemas['ModuleState'][]>()
const loadFailed = ref(false)
const busy = ref(false)
const passwordSet = ref(false)
let timer: number | undefined

const loadText = computed(() => t(loadFailed.value ? 'home.loadFailed' : 'app.loading'))
const moduleName = (id: string) => (te(`modules.names.${id}`) ? t(`modules.names.${id}`) : id)

// Saved values are undefined until loaded; their fields show only after that.
const askSaved = ref<boolean>()
const ask = useDrawerSetting(askSaved, (v) => api(client.PUT('/api/admin/general', { body: { ask_before_close: v! } })))

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

async function run(action: () => Promise<unknown>) {
  busy.value = true
  try {
    await action()
    await refresh()
  } catch (e) {
    loadFailed.value = true
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = false
  }
}

async function refreshTwitch() {
  overview.value = await api(client.GET('/api/admin/twitch'))
}

async function refresh() {
  await refreshTwitch()
  modules.value = await api(client.GET('/api/admin/modules'))
}

async function load() {
  const [general, obs] = await Promise.all([api(client.GET('/api/admin/general')), api(client.GET('/api/admin/obs'))])
  askSaved.value = general.ask_before_close
  Object.assign(savedAddress, { host: obs.host, port: obs.port })
  hostSaved.value = obs.host
  portSaved.value = String(obs.port)
  passwordSet.value = obs.password_set
}

const loaded = () => modules.value !== undefined && hostSaved.value !== undefined

const toggleModule = (m: Schemas['ModuleState']) =>
  run(() =>
    api(client.PUT('/api/admin/modules/{id}', { params: { path: { id: m.id } }, body: { enabled: !m.enabled } })),
  )
const removePassword = () =>
  run(async () => {
    await putObs({ password: '' })
    passwordSet.value = false
  })

onMounted(async () => {
  await run(load)
  // Polled so a sign-in finished in the browser shows up; until the first load succeeds, retry all of it.
  timer = window.setInterval(() => {
    const poll = loaded() ? refreshTwitch() : load().then(refresh)
    poll.catch(() => {})
  }, 2000)
})
onBeforeUnmount(() => window.clearInterval(timer))
</script>

<style scoped>
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
