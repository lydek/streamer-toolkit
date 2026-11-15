<template>
  <FullPage :title="t('modules.names.chat')">
    <TemplateEditor />
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'
import TemplateEditor from './TemplateEditor.vue'

const { t } = useI18n()
</script>
