<template>
  <FullPage :title="t('modules.names.screen-tools')" tile="screen-tools" :status="status">
    <section class="stack" aria-labelledby="break-heading">
      <div class="sec-head">
        <h2 id="break-heading">{{ t('screenTools.break.heading') }}</h2>
        <ObsStatusLink overlay="/overlay/break/" />
      </div>
      <BreakCardSettings :card="live.breakCard" @error="report" />
    </section>

    <section class="stack" aria-labelledby="warnings-heading">
      <div class="sec-head">
        <h2 id="warnings-heading">{{ t('screenTools.warnings.heading') }}</h2>
        <ObsStatusLink overlay="/overlay/warnings/" />
      </div>
      <WarningSettings :warnings="live.warnings" :inputs="live.obsState?.inputs ?? []" />
    </section>

    <section class="stack" aria-labelledby="appearance-heading">
      <h2 id="appearance-heading">{{ t('screenTools.appearance.heading') }}</h2>
      <AppearanceSettings :appearance="live.appearance" />
    </section>

    <section class="stack" aria-labelledby="media-heading">
      <h2 id="media-heading">{{ t('screenTools.media.heading') }}</h2>
      <MediaRules :media="media" @error="report" />
      <h3>{{ t('screenTools.media.library') }}</h3>
      <MediaLibrary
        @error="report"
        @loaded="(items) => (media = items)"
        @imported="(n) => toast.add(successToast(t('screenTools.media.imported', { n })))"
      />
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { computed, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas } from './api'
import AppearanceSettings from './AppearanceSettings.vue'
import BreakCardSettings from './BreakCardSettings.vue'
import FullPage from './FullPage.vue'
import MediaLibrary from './MediaLibrary.vue'
import MediaRules from './MediaRules.vue'
import { errorToast, successToast, useLiveEvents } from './home'
import WarningSettings from './WarningSettings.vue'
import ObsStatusLink from './ObsStatusLink.vue'

const { t } = useI18n()
const toast = useToast()
const live = useLiveEvents()
const media = shallowRef<Schemas['MediaItem'][]>([])
const status = computed(() => {
  const card = live.value.breakCard
  if (!card) return undefined
  return t(card.shown ? 'screenTools.break.shown' : 'screenTools.break.hidden')
})

const report = (e: unknown) =>
  toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))

</script>

<style scoped>
.sec-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
}

.sec-head h2 {
  margin: 0;
}
</style>
