<template>
  <AppDrawer id="scene" :title="title" full-page="/scenes">
    <template #actions>
      <DrawerNotice />
    </template>
    <template #now>
      <SceneList v-if="obsReady(live.obsState)" :state="live.obsState" @error="failed" />
      <p v-else>{{ t('scene.notConnected') }}</p>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import type { LiveState } from './home'
import { useHomeDrawer } from './homeDrawer'
import SceneList from './SceneList.vue'
import { obsReady } from './scenes'

defineProps<{ title: string; live: LiveState }>()
const { t } = useI18n()
const { failed } = useHomeDrawer()
</script>
