<template>
  <!-- 影片與 GIF: the media library and what each reward plays. No OBS status: media plays on the alerts overlay. -->
  <FullPage :title="t('nav.categories.pictures')" tile="screen-tools">
    <MediaRules :media="media" @error="report" />
    <h2>{{ t('screenTools.media.library') }}</h2>
    <MediaLibrary
      @error="report"
      @loaded="(items) => (media = items)"
      @imported="(n) => toast.add(successToast(t('screenTools.media.imported', { n })))"
    />
  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'
import MediaLibrary from './MediaLibrary.vue'
import MediaRules from './MediaRules.vue'

const { t } = useI18n()
const toast = useToast()
const media = shallowRef<Schemas['MediaItem'][]>([])
const report = (e: unknown) =>
  toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
</script>
