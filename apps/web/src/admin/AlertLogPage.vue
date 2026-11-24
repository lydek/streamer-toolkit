<template>
  <!-- 最近事件: this launch's alerts with what became of them. -->
  <FullPage tile="alerts" overlay="alerts">
    <RecentAlerts />
  </FullPage>
</template>

<script setup lang="ts">
import FullPage from './FullPage.vue'
import RecentAlerts from './RecentAlerts.vue'

</script>
