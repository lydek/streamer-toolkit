<template>
  <main>
    <h1>{{ t('app.name') }}</h1>

    <section aria-labelledby="twitch-heading">
      <h2 id="twitch-heading">{{ t('twitch.heading') }}</h2>

      <p v-if="!overview">{{ t('app.loading') }}</p>

      <form v-else-if="!overview.client_id || editingClientId" @submit.prevent="saveClientId">
        <i18n-t keypath="twitch.guide" tag="p">
          <template #url><code>http://localhost</code></template>
        </i18n-t>
        <label for="client-id">{{ t('twitch.clientId') }}</label>
        <input id="client-id" v-model="clientIdInput" autocomplete="off" required />
        <button type="submit" :disabled="busy">{{ t('twitch.save') }}</button>
        <button v-if="overview.client_id" type="button" @click="editingClientId = false">{{ t('twitch.cancel') }}</button>
      </form>

      <template v-else>
        <div v-if="overview.status.state === 'signed_in'">
          <p>{{ t('twitch.signedIn') }}<strong>{{ overview.status.login }}</strong></p>
          <button type="button" :disabled="busy" @click="logout">{{ t('twitch.logout') }}</button>
        </div>

        <div v-else-if="overview.status.state === 'pending'">
          <p>{{ t('twitch.enterCode') }}</p>
          <p class="user-code" aria-live="polite">{{ overview.status.user_code }}</p>
          <button type="button" @click="openActivate">{{ t('twitch.openActivate') }}</button>
        </div>

        <div v-else>
          <p v-if="overview.status.state === 'failed'" role="alert">{{ errorMessage(overview.status.code) }}</p>
          <button type="button" :disabled="busy" @click="login">{{ t('twitch.login') }}</button>
        </div>

        <button type="button" @click="startEditClientId">{{ t('twitch.changeClientId') }}</button>
      </template>

      <p v-if="error" role="alert">{{ error }}</p>
    </section>

    <section aria-labelledby="modules-heading">
      <h2 id="modules-heading">{{ t('modules.heading') }}</h2>
      <ul>
        <li v-for="m in modules" :key="m.id">
          <label>
            <input type="checkbox" :checked="m.enabled" :disabled="busy" @change="toggleModule(m)" />
            {{ te(`modules.names.${m.id}`) ? t(`modules.names.${m.id}`) : m.id }}
          </label>
        </li>
      </ul>
    </section>

    <section aria-labelledby="simulate-heading">
      <h2 id="simulate-heading">{{ t('simulate.heading') }}</h2>
      <p>{{ t('simulate.hint') }}</p>
      <form @submit.prevent="simulateChat">
        <label for="sim-user">{{ t('simulate.userName') }}</label>
        <input id="sim-user" v-model="simUser" autocomplete="off" maxlength="25" />
        <label for="sim-text">{{ t('simulate.text') }}</label>
        <input id="sim-text" v-model="simText" autocomplete="off" maxlength="500" required />
        <button type="submit" :disabled="busy">{{ t('simulate.send') }}</button>
      </form>
      <p aria-live="polite">{{ simSent ? t('simulate.sent') : '' }}</p>
    </section>
  </main>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'

const { t, te } = useI18n()

const overview = ref<Schemas['Overview']>()
const modules = ref<Schemas['ModuleState'][]>([])
const clientIdInput = ref('')
const editingClientId = ref(false)
const busy = ref(false)
const error = ref('')
const simUser = ref('')
const simText = ref('')
const simSent = ref(false)
let timer: number | undefined

async function refresh() {
  overview.value = await api(client.GET('/api/admin/twitch'))
  modules.value = await api(client.GET('/api/admin/modules'))
}

async function run(action: () => Promise<unknown>) {
  busy.value = true
  error.value = ''
  try {
    await action()
    await refresh()
  } catch (e) {
    error.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  } finally {
    busy.value = false
  }
}

const saveClientId = () =>
  run(async () => {
    await api(client.PUT('/api/admin/twitch/client-id', { body: { client_id: clientIdInput.value } }))
    editingClientId.value = false
  })
const login = () => run(() => api(client.POST('/api/admin/twitch/login')))
const logout = () => run(() => api(client.POST('/api/admin/twitch/logout')))
const toggleModule = (m: Schemas['ModuleState']) =>
  run(() =>
    api(client.PUT('/api/admin/modules/{id}', { params: { path: { id: m.id } }, body: { enabled: !m.enabled } })),
  )
const simulateChat = () =>
  run(async () => {
    simSent.value = false
    await api(client.POST('/api/admin/simulate/chat', { body: { userName: simUser.value, text: simText.value } }))
    simText.value = ''
    simSent.value = true
  })

async function openActivate() {
  try {
    await invoke('open_twitch_activate')
  } catch (code) {
    error.value = errorMessage(String(code))
  }
}

function startEditClientId() {
  clientIdInput.value = overview.value?.client_id ?? ''
  editingClientId.value = true
}

onMounted(async () => {
  await run(async () => {})
  // Poll while a device login is pending so the page updates once the user approves.
  timer = window.setInterval(() => {
    if (overview.value?.status.state === 'pending') refresh().catch(() => {})
  }, 2000)
})
onBeforeUnmount(() => window.clearInterval(timer))
</script>
