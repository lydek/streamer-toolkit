<template>
  <AppDrawer id="rewards" :title="title" full-page="/rewards">
    <template #now>
      <p>{{ t('rewards.tile') }}</p>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'

defineProps<{ title: string }>()
const { t } = useI18n()
</script>
