<template>
  <main>
    <p>{{ t('app.notFound') }}</p>
    <RouterLink to="/">{{ t('app.home') }}</RouterLink>
  </main>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
</script>
