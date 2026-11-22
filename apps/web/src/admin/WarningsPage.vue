<template>
  <!-- 警語: the warnings overlay's checks and settings. -->
  <FullPage :title="t('nav.categories.widgets')" tile="screen-tools" overlay="warnings">
    <WarningSettings :warnings="live.warnings" :inputs="live.obsState?.inputs ?? []" />
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'
import { useLiveEvents } from './home'
import WarningSettings from './WarningSettings.vue'

const { t } = useI18n()
const live = useLiveEvents()
</script>
