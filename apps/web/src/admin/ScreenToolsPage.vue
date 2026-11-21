<template>
  <FullPage :title="t('modules.names.screen-tools')" tile="screen-tools" :status="status">
    <section class="stack" aria-labelledby="break-heading">
      <div class="sec-head">
        <h2 id="break-heading">{{ t('screenTools.break.heading') }}</h2>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('break')" />
      </div>
      <BreakCardSettings :card="live.breakCard" @error="report" />
    </section>

    <section class="stack" aria-labelledby="warnings-heading">
      <div class="sec-head">
        <h2 id="warnings-heading">{{ t('screenTools.warnings.heading') }}</h2>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('warnings')" />
      </div>
      <WarningSettings :warnings="live.warnings" :inputs="live.obsState?.inputs ?? []" />
    </section>

    <section class="stack" aria-labelledby="appearance-heading">
      <h2 id="appearance-heading">{{ t('screenTools.appearance.heading') }}</h2>
      <AppearanceSettings :appearance="live.appearance" />
    </section>

    <section class="stack" aria-labelledby="media-heading">
      <h2 id="media-heading">{{ t('screenTools.media.heading') }}</h2>
      <MediaLibrary @error="report" @imported="(n) => toast.add(successToast(t('screenTools.media.imported', { n })))" />
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import AppearanceSettings from './AppearanceSettings.vue'
import BreakCardSettings from './BreakCardSettings.vue'
import FullPage from './FullPage.vue'
import MediaLibrary from './MediaLibrary.vue'
import { errorToast, successToast, useLiveEvents } from './home'
import WarningSettings from './WarningSettings.vue'

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

async function copyObsUrl(overlay: 'break' | 'warnings') {
  try {
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/${overlay}/`)
    toast.add(successToast(t('chat.copied')))
  } catch {
    report(t('chat.copyFailed'))
  }
}
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
