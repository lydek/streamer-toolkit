<template>
  <!-- Twitch Client ID and sign-in, in the connections drawer and in Settings → Twitch. -->
  <p v-if="!overview">{{ t('app.loading') }}</p>

  <form v-else-if="!overview.client_id || editingClientId" @submit.prevent="saveClientId">
    <i18n-t keypath="twitch.guide" tag="p">
      <template #url><code>http://localhost</code></template>
    </i18n-t>
    <label for="client-id">{{ t('twitch.clientId') }}</label>
    <InputText id="client-id" ref="primary" v-model="clientIdInput" autocomplete="off" required />
    <Button type="submit" :label="t('twitch.save')" :disabled="busy" />
    <Button v-if="overview.client_id" :label="t('twitch.cancel')" outlined @click="cancelEdit" />
  </form>

  <template v-else>
    <div v-if="overview.status.state === 'signed_in'" class="stack">
      <p>{{ t('twitch.signedIn') }}<strong>{{ overview.status.login }}</strong></p>
      <Button ref="primary" :label="t('twitch.logout')" icon="pi pi-sign-out" outlined :disabled="busy" @click="logout" />
    </div>

    <div v-else-if="overview.status.state === 'pending'" class="stack">
      <p>{{ t('twitch.enterCode') }}</p>
      <p class="user-code" aria-live="polite">{{ overview.status.user_code }}</p>
      <Button ref="primary" :label="t('twitch.openActivate')" icon="pi pi-external-link" @click="openActivate" />
    </div>

    <div v-else class="stack">
      <p v-if="overview.status.state === 'failed'" role="alert">{{ errorMessage(overview.status.code) }}</p>
      <Button ref="primary" :label="t('twitch.login')" icon="pi pi-sign-in" :disabled="busy" @click="login" />
    </div>

    <Button :label="t('twitch.changeClientId')" text @click="startEditClientId" />
  </template>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { keepFocus } from './page'

// `run` is the parent's action runner: it sets `busy`, refreshes `overview` afterwards and reports failures.
const props = defineProps<{
  overview?: Schemas['Overview']
  busy: boolean
  run: (action: () => Promise<unknown>) => Promise<void>
}>()

const { t } = useI18n()
const clientIdInput = ref('')
const editingClientId = ref(false)
/** The main control of whichever state shows; it takes focus when the one used went away. */
const primary = ref<{ $el: HTMLElement }>()
const refocus = () => keepFocus(() => primary.value?.$el)

/** Each action replaces the control it was started from (the form, or sign in with sign out). */
const act = async (action: () => Promise<unknown>) => {
  await props.run(action)
  await refocus()
}
const saveClientId = () =>
  act(async () => {
    await api(client.PUT('/api/admin/twitch/client-id', { body: { client_id: clientIdInput.value } }))
    editingClientId.value = false
  })
const login = () => act(() => api(client.POST('/api/admin/twitch/login')))
const logout = () => act(() => api(client.POST('/api/admin/twitch/logout')))
function cancelEdit() {
  editingClientId.value = false
  void refocus()
}
// The command rejects with an error code, reported like an API error.
const openActivate = () =>
  props.run(() =>
    invoke('open_twitch_activate').catch((code) => {
      throw new ApiError(String(code))
    }),
  )

function startEditClientId() {
  clientIdInput.value = props.overview?.client_id ?? ''
  editingClientId.value = true
  void refocus()
}
</script>

<style scoped>
.user-code {
  font-size: 1.5rem;
  font-weight: 600;
  letter-spacing: 0.1em;
}
</style>
