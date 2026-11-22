<template>
  <!-- 影片與 GIF: the media files and the rewards each is used by (set on 兌換時播放). No OBS status: media plays on
       the alerts overlay. -->
  <FullPage :title="t('nav.categories.pictures')" tile="screen-tools">
    <MediaLibrary
      :used-by="usedBy"
      @error="report"
      @imported="(n) => toast.add(successToast(t('screenTools.media.imported', { n })))"
    />
  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { computed, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'
import MediaLibrary from './MediaLibrary.vue'

const { t } = useI18n()
const toast = useToast()
const rules = shallowRef<Schemas['MediaRuleBody'][]>([])
const rewards = shallowRef<Schemas['Reward'][]>([])
// Each file's rewards by title (a reward Twitch no longer lists shows by its ID).
const usedBy = computed(() => {
  const titles = new Map(rewards.value.map((r) => [r.id, r.title]))
  const map = new Map<string, string[]>()
  for (const r of rules.value) map.set(r.media_id, [...(map.get(r.media_id) ?? []), titles.get(r.reward_id) ?? r.reward_id])
  return map
})
void api(client.GET('/api/admin/screen-tools/media-rules')).then((r) => (rules.value = r), () => {})
void api(client.GET('/api/admin/checkin/rewards')).then((r) => (rewards.value = r), () => {})
const report = (e: unknown) =>
  toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
</script>
