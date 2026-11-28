<template>
  <AppDrawer id="image-sizes" :title="title" full-page="/image-sizes">
    <template #now>
      <p>{{ t('imageSizes.tile') }}</p>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'

defineProps<{ title: string }>()
const { t } = useI18n()
</script>
