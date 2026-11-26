<template>
  <!-- Sending as the streamer (user:write:chat), in the docked chat and on the chat log page. Without the scope it
       offers to sign in again for it; signed out, it waits for a sign-in. -->
  <div class="composer">
    <slot />
    <Button v-if="needsScope" :label="t('dock.grant')" icon="pi pi-lock" severity="secondary" class="grow" @click="grant" />
    <template v-else>
      <InputText
        v-model="text"
        class="grow"
        :maxlength="MAX"
        :disabled="!signedIn || busy"
        :placeholder="t(signedIn ? 'dock.sendPlaceholder' : 'dock.signInFirst')"
        :aria-label="t('dock.send')"
        @keydown.enter.prevent="send"
      />
      <Button icon="pi pi-send" :disabled="!signedIn || busy || !text.trim()" :aria-label="t('dock.send')" @click="send" />
    </template>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { errorMessage } from '../i18n'
import { ApiError, api, client } from './api'
import { errorToast, twitchOverview } from './home'
import { requestScopes } from './scopes'

// Twitch's limit for one chat message, checked again by the Host.
const MAX = 500
const SCOPE = 'user:write:chat'

const { t } = useI18n()
const toast = useToast()
const router = useRouter()
const text = ref('')
const busy = ref(false)
const signedIn = computed(() => twitchOverview.value?.status.state === 'signed_in')
const needsScope = computed(() => signedIn.value && !twitchOverview.value?.scopes?.includes(SCOPE))

const report = (e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))

async function send() {
  const message = text.value.trim()
  if (!message || busy.value) return
  busy.value = true
  try {
    await api(client.POST('/api/admin/chat/send', { body: { text: message } }))
    text.value = ''
  } catch (e) {
    report(e)
  } finally {
    busy.value = false
  }
}

async function grant() {
  try {
    await requestScopes([SCOPE], router)
  } catch (e) {
    report(e)
  }
}
</script>

<style scoped>
.composer {
  display: flex;
  gap: 0.375rem;
  align-items: center;
}

.grow {
  flex: 1;
  min-width: 0;
}
</style>
