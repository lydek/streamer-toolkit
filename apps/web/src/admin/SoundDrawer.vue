<template>
  <AppDrawer id="sound" :title="title" full-page="/sounds">
    <template #actions>
      <ObsStatusLink overlay="/overlay/sound/" />
      <DrawerNotice />
    </template>
    <template #now>
      <p v-if="sounds.state === 'failed'" role="alert">{{ t('sound.loadFailed') }}</p>
      <p v-else-if="sounds.state === 'loading'">{{ t('app.loading') }}</p>
      <template v-else-if="sounds.state === 'ready'">
        <p v-if="!sounds.items.length">{{ t('sound.empty') }}</p>
        <ul v-else class="sound-list">
          <li v-for="s in sounds.items" :key="s.id">{{ s.label }}</li>
        </ul>
      </template>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { Schemas } from './api'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import ObsStatusLink from './ObsStatusLink.vue'

defineProps<{ title: string; sounds: { state: 'loading' | 'failed' } | { state: 'ready'; items: Schemas['Sound'][] } }>()
const { t } = useI18n()
</script>

<style scoped>
.sound-list {
  margin: 0;
  padding: 0;
  list-style: none;
  overflow-wrap: anywhere;
}
</style>
