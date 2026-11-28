<template>
  <AppDrawer id="redemption" :title="title" full-page="/redemptions">
    <template #now>
      <p v-if="recent.state === 'failed'" role="alert">{{ t('redemption.loadFailed') }}</p>
      <p v-else-if="recent.state === 'loading'">{{ t('app.loading') }}</p>
      <RedemptionList v-else-if="recent.state === 'ready'" :records="recent.items" :empty="t('redemption.empty')" />
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import RedemptionList from './RedemptionList.vue'
import type { RedemptionRecord } from './redemptions'

defineProps<{ title: string; recent: { state: 'loading' | 'failed' } | { state: 'ready'; items: RedemptionRecord[] } }>()
const { t } = useI18n()
</script>
