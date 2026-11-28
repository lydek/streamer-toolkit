<template>
  <AppDrawer id="songs" :title="title" full-page="/songs">
    <template #now>
      <p>{{ t('songs.tile') }}</p>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'

defineProps<{ title: string }>()
const { t } = useI18n()
</script>
