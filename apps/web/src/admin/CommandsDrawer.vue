<template>
  <AppDrawer id="commands" :title="title" full-page="/commands">
    <template #now>
      <p>{{ t('bot.tile') }}</p>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'

defineProps<{ title: string }>()
const { t } = useI18n()
</script>
