<template>
  <FullPage :title="t('pages.chatTemplates.name')" tile="chat" overlay="chat">
    <ChatPreview />
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'
import ChatPreview from './ChatPreview.vue'

const { t } = useI18n()
</script>
