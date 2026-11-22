<template>
  <!-- 休息卡片, the first of the screen widget pages (警語 is WarningsPage.vue). -->
  <FullPage :title="t('modules.names.screen-tools')" tile="screen-tools" overlay="break" :status="status">
    <BreakCardSettings :card="live.breakCard" @error="report" />

    <section class="stack" aria-labelledby="appearance-heading">
      <h2 id="appearance-heading">{{ t('screenTools.appearance.heading') }}</h2>
      <AppearanceSettings :appearance="live.appearance" />
    </section>

  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import AppearanceSettings from './AppearanceSettings.vue'
import BreakCardSettings from './BreakCardSettings.vue'
import FullPage from './FullPage.vue'
import { errorToast, useLiveEvents } from './home'

const { t } = useI18n()
const toast = useToast()
const live = useLiveEvents()
const status = computed(() => {
  const card = live.value.breakCard
  if (!card) return undefined
  return t(card.shown ? 'screenTools.break.shown' : 'screenTools.break.hidden')
})

const report = (e: unknown) =>
  toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))

</script>
