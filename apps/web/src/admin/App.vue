<template>
  <main>
    <h1>Streamer Toolkit</h1>

    <section aria-labelledby="twitch-heading">
      <h2 id="twitch-heading">Twitch 連線</h2>

      <p v-if="!overview">載入中…</p>

      <form v-else-if="!overview.client_id || editingClientId" @submit.prevent="saveClientId">
        <p>
          請先到 Twitch Developer Console 註冊一個 app：類別選「Public」，OAuth Redirect URL 填
          <code>http://localhost</code>，再把產生的 Client ID 貼到這裡。
        </p>
        <label for="client-id">Client ID</label>
        <input id="client-id" v-model="clientIdInput" autocomplete="off" required />
        <button type="submit" :disabled="busy">儲存</button>
        <button v-if="overview.client_id" type="button" @click="editingClientId = false">取消</button>
      </form>

      <template v-else>
        <div v-if="overview.status.state === 'signed_in'">
          <p>已登入：<strong>{{ overview.status.login }}</strong></p>
          <button type="button" :disabled="busy" @click="logout">登出</button>
        </div>

        <div v-else-if="overview.status.state === 'pending'">
          <p>請在 Twitch 驗證頁輸入以下代碼：</p>
          <p class="user-code" aria-live="polite">{{ overview.status.user_code }}</p>
          <button type="button" @click="openActivate">開啟 Twitch 驗證頁</button>
        </div>

        <div v-else>
          <p v-if="overview.status.state === 'failed'" role="alert">{{ message(overview.status.code) }}</p>
          <button type="button" :disabled="busy" @click="login">登入 Twitch</button>
        </div>

        <button type="button" @click="startEditClientId">變更 Client ID</button>
      </template>

      <p v-if="error" role="alert">{{ error }}</p>
    </section>
  </main>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { ApiError, api } from './api'

type Status =
  | { state: 'signed_out' }
  | { state: 'pending'; user_code: string; verification_uri: string }
  | { state: 'signed_in'; user_id: string; login: string }
  | { state: 'failed'; code: string }

interface Overview {
  client_id: string
  status: Status
}

// Error codes come from the Host; messages move to vue-i18n with the skeleton foundations.
const messages: Record<string, string> = {
  invalid_client_id: 'Client ID 格式不正確（只能是英文字母與數字）。',
  'twitch.client_id_required': '請先設定 Client ID。',
  'twitch.sign_in_expired': '驗證碼已過期，請重新登入。',
  'twitch.sign_in_refused': 'Twitch 拒絕了登入，請確認 Client ID 與 app 類別（Public）。',
  'twitch.unreachable': '連不到 Twitch，請檢查網路。',
}
const message = (code: string) => messages[code] ?? `發生錯誤（${code}）`

const overview = ref<Overview>()
const clientIdInput = ref('')
const editingClientId = ref(false)
const busy = ref(false)
const error = ref('')
let timer: number | undefined

async function refresh() {
  overview.value = await api<Overview>('/api/admin/twitch')
}

async function run(action: () => Promise<unknown>) {
  busy.value = true
  error.value = ''
  try {
    await action()
    await refresh()
  } catch (e) {
    error.value = e instanceof ApiError ? message(e.code) : String(e)
  } finally {
    busy.value = false
  }
}

const saveClientId = () =>
  run(async () => {
    await api('/api/admin/twitch/client-id', { method: 'PUT', body: JSON.stringify({ client_id: clientIdInput.value }) })
    editingClientId.value = false
  })
const login = () => run(() => api('/api/admin/twitch/login', { method: 'POST' }))
const logout = () => run(() => api('/api/admin/twitch/logout', { method: 'POST' }))
const openActivate = () => invoke('open_twitch_activate')

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
