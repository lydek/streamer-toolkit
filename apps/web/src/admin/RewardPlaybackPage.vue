<template>
  <!-- 兌換時播放: what each reward plays on a redemption, the sound and the video or GIF; the files themselves are
       on the sound and media pages. -->
  <FullPage :title="t('nav.categories.points')">
    <div class="sources">
      <span v-if="!hidden.has('/sounds')">{{ t('rewardPlayback.soundOverlay') }}<ObsStatusLink overlay="/overlay/sound/" /></span>
      <span v-if="!hidden.has('/media')">{{ t('rewardPlayback.mediaOverlay') }}<ObsStatusLink overlay="/overlay/alerts/" /></span>
    </div>
    <section v-if="!hidden.has('/sounds')" class="stack" aria-labelledby="reward-sounds">
      <h2 id="reward-sounds">{{ t('sound.rewards') }}</h2>
      <p v-if="rewardError">{{ rewardError }}</p>
      <SoundSlots
        v-if="library && rewardSlots.length"
        :key="rewardSlots.map((r) => r.slot).join()"
        :slots="rewardSlots"
        playable
        @played="toast.add(successToast(t('sound.playedInObs')))"
        @error="report"
      />
    </section>
    <section v-if="!hidden.has('/media')" class="stack" aria-labelledby="reward-media">
      <h2 id="reward-media">{{ t('screenTools.media.heading') }}</h2>
      <MediaRules :media="media" @error="report" />
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { useModules } from './features'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'
import MediaRules from './MediaRules.vue'
import ObsStatusLink from './ObsStatusLink.vue'
import SoundSlots from './SoundSlots.vue'

const { t } = useI18n()
const toast = useToast()
const { hidden } = useModules()
const library = shallowRef<Schemas['SoundLibrary']>()
const media = shallowRef<Schemas['MediaItem'][]>([])
const rewards = ref<Schemas['Reward'][]>([])
const rewardError = ref('')
const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))
const report = (e: unknown) => toast.add(errorToast(message(e)))

// The channel's rewards, plus rewards with a sound that Twitch no longer lists (shown by ID).
const rewardSlots = computed(() => {
  const titles = new Map(rewards.value.map((r) => [r.id, r.title]))
  const ids = [
    ...rewards.value.map((r) => r.id),
    ...Object.keys(library.value?.slots ?? {}).flatMap((s) => (s.startsWith('reward.') ? [s.slice(7)] : [])),
  ]
  return [...new Set(ids)].map((id) => ({ slot: `reward.${id}`, label: titles.get(id) ?? id }))
})

void api(client.GET('/api/admin/sounds')).then((l) => (library.value = l), report)
void api(client.GET('/api/admin/screen-tools/media')).then((m) => (media.value = m), report)
void api(client.GET('/api/admin/checkin/rewards')).then(
  (r) => (rewards.value = r),
  (e: unknown) => (rewardError.value = message(e)),
)
</script>

<style scoped>
.sources {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1.5rem;
  align-items: center;
  margin-bottom: 1rem;
}

.sources span {
  display: inline-flex;
  gap: 0.5rem;
  align-items: center;
}
</style>
