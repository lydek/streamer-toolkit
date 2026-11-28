<template>
  <AppDrawer id="connections" :title="t('home.connections')">
    <template #now>
      <DrawerNotice />
      <h4>{{ t('twitch.heading') }}</h4>
      <TwitchSignIn :overview="overview" :busy="busy" :run="run" />

      <h4>OBS</h4>
      <p>
        {{ t(obsLight(live).key) }}
        <template v-if="live.obs?.reason">— {{ errorMessage(live.obs.reason) }}</template>
      </p>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import type { Schemas } from './api'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import { type LiveState, obsLight } from './home'
import { useHomeDrawer } from './homeDrawer'
import TwitchSignIn from './TwitchSignIn.vue'

defineProps<{ overview?: Schemas['Overview']; live: LiveState }>()
const { t } = useI18n()
const { busy, run } = useHomeDrawer()
</script>

<style scoped>
h4 {
  margin: 0;
}
</style>
