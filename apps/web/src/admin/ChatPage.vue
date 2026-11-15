<template>
  <FullPage :title="t('modules.names.chat')">
    <ChatPreview />
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'
import ChatPreview from './ChatPreview.vue'

const { t } = useI18n()
</script>
