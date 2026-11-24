<template>
  <!-- 警語: the warnings overlay's checks and settings. -->
  <FullPage tile="screen-tools" overlay="warnings">
    <WarningSettings :warnings="live.warnings" :inputs="live.obsState?.inputs ?? []" />
  </FullPage>
</template>

<script setup lang="ts">
import FullPage from './FullPage.vue'
import { useLiveEvents } from './home'
import WarningSettings from './WarningSettings.vue'

const live = useLiveEvents()
</script>
