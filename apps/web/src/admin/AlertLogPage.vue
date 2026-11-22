<template>
  <!-- 最近事件: this launch's alerts with what became of them. -->
  <FullPage :title="t('modules.names.alerts')" tile="alerts" overlay="alerts">
    <RecentAlerts />
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'
import RecentAlerts from './RecentAlerts.vue'

const { t } = useI18n()
</script>
