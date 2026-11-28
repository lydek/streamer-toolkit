<template>
  <AppDrawer id="chat" :title="title" full-page="/chat">
    <template #actions>
      <ObsStatusLink overlay="/overlay/chat/" />
      <form @submit.prevent="sendChat">
        <p>{{ t('simulate.hint') }}</p>
        <label for="sim-user">{{ t('simulate.userName') }}</label>
        <InputText id="sim-user" v-model="simUser" autocomplete="off" maxlength="25" />
        <label for="sim-text">{{ t('simulate.text') }}</label>
        <InputText id="sim-text" v-model="simText" autocomplete="off" maxlength="500" required />
        <Button type="submit" :label="t('simulate.send')" icon="pi pi-send" :disabled="busy" />
      </form>
      <DrawerNotice />
    </template>
    <template #now>
      <ChatNow v-model:log="log" :broadcaster-id="broadcasterId" :disconnected="disconnected" />
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import type { ChatLog } from './chatLog'
import ChatNow from './ChatNow.vue'
import DrawerNotice from './DrawerNotice.vue'
import { useHomeDrawer } from './homeDrawer'
import ObsStatusLink from './ObsStatusLink.vue'
import { simulateChat } from './simulate'

defineProps<{ title: string; broadcasterId?: string; disconnected: boolean }>()
// Kept by the home page, so messages keep arriving while the drawer is closed.
const log = defineModel<ChatLog>('log', { required: true })
const { t } = useI18n()
const { busy, run, notify } = useHomeDrawer()
const simUser = ref('')
const simText = ref('')

const sendChat = () =>
  run(async () => {
    await simulateChat({ userName: simUser.value, text: simText.value })
    simText.value = ''
    notify(t('simulate.sent'), false)
  })
</script>
