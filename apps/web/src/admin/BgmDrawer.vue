<template>
  <AppDrawer id="bgm" :title="title" full-page="/music">
    <template #actions>
      <ObsStatusLink overlay="/overlay/bgm/" />
      <DrawerNotice />
    </template>
    <template #now>
      <MusicPlayer :event="live.bgm" :playlists="playlists" @error="failed" />
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import type { Schemas } from './api'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import type { LiveState } from './home'
import { useHomeDrawer } from './homeDrawer'
import MusicPlayer from './MusicPlayer.vue'
import ObsStatusLink from './ObsStatusLink.vue'

defineProps<{ title: string; live: LiveState; playlists: Schemas['MusicPlaylist'][] }>()
const { failed } = useHomeDrawer()
</script>
