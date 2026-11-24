<template>
  <!-- 兌換時播放: one table for what each reward plays on a redemption (sound + video/GIF); the files themselves are
       on the sound and media pages. -->
  <FullPage :status="t('rewardPlayback.summary')">
    <div class="sources">
      <span v-if="showSounds">{{ t('rewardPlayback.soundOverlay') }}<ObsStatusLink overlay="/overlay/sound/" /></span>
      <span v-if="showMedia">{{ t('rewardPlayback.mediaOverlay') }}<ObsStatusLink overlay="/overlay/alerts/" /></span>
    </div>

    <p v-if="rewardError" role="alert">{{ rewardError }}</p>
    <p v-if="loading && !rewardError">{{ t('app.loading') }}</p>
    <p v-else-if="!rows.length && !rewardError">{{ t('screenTools.rules.noRewards') }}</p>

    <div v-if="rows.length" class="tbl-wrap">
      <table class="table playback-table">
        <thead>
          <tr>
            <th scope="col">{{ t('screenTools.rules.reward') }}</th>
            <th v-if="showSounds" scope="col">{{ t('rewardPlayback.sound') }}</th>
            <th v-if="showMedia" scope="col">{{ t('screenTools.rules.media') }}</th>
            <th v-if="showMedia" scope="col">{{ t('screenTools.rules.position') }}</th>
            <th v-if="showMedia" scope="col">{{ t('rewardPlayback.videoVolume') }}</th>
            <th scope="col"><span class="sr-only">{{ t('screenTools.media.actions') }}</span></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rows" :key="row.id" :data-reward="row.id">
            <th :id="`reward-name-${row.id}`" scope="row">
              {{ row.title }}
              <div v-if="row.isCheckin && canTest(row.id)" class="muted checkin-note">{{ t('rewardPlayback.checkinManaged') }}</div>
            </th>
            <template v-if="row.isCheckin && !canTest(row.id)">
              <td :colspan="controlCols"><span class="muted">{{ t('rewardPlayback.checkinManaged') }}</span></td>
              <td class="end"></td>
            </template>
            <template v-else>
              <td v-if="showSounds">
                <Select
                  :model-value="soundOf(row.id)"
                  :options="soundOptions"
                  option-label="label"
                  option-value="value"
                  :aria-labelledby="`reward-name-${row.id}`"
                  @update:model-value="(v: string) => setSound(row.id, v)"
                />
              </td>
              <td v-if="showMedia">
                <Select
                  :model-value="ruleOf(row.id)?.media_id ?? null"
                  :options="media"
                  option-label="title"
                  option-value="id"
                  :placeholder="t('screenTools.rules.none')"
                  :aria-label="t('screenTools.rules.mediaFor', { reward: row.title })"
                  show-clear
                  @update:model-value="(id: string | null) => setMedia(row.id, id)"
                />
              </td>
              <td v-if="showMedia">
                <Select
                  :model-value="ruleOf(row.id)?.position ?? 'bottom_right'"
                  :options="POSITIONS"
                  :option-label="(p: Position) => t(`screenTools.rules.positions.${p}`)"
                  :aria-label="t('screenTools.rules.positionFor', { reward: row.title })"
                  :disabled="!ruleOf(row.id)"
                  @update:model-value="(position: Position) => saveRule(row.id, { position })"
                />
              </td>
              <td v-if="showMedia">
                <span v-if="!ruleOf(row.id)" class="muted">—</span>
                <span v-else-if="isGif(row.id)" class="muted">{{ t('rewardPlayback.gifNoAudio') }}</span>
                <span v-else class="volume">
                  <Checkbox
                    :model-value="ruleOf(row.id)!.volume == null"
                    :input-id="`follow-${row.id}`"
                    binary
                    @update:model-value="(follow: boolean) => saveRule(row.id, { volume: follow ? null : 100 })"
                  />
                  <label :for="`follow-${row.id}`">{{ t('screenTools.rules.followMaster') }}</label>
                  <InputNumber
                    v-if="ruleOf(row.id)!.volume != null"
                    :model-value="ruleOf(row.id)!.volume"
                    :min="0"
                    :max="100"
                    suffix="%"
                    :aria-label="t('screenTools.rules.volumeFor', { reward: row.title })"
                    @update:model-value="(v: number | null) => v !== null && saveRule(row.id, { volume: v })"
                  />
                </span>
              </td>
              <td class="end">
                <Button
                  :label="t(testing === row.id ? 'sound.stopInObs' : 'sound.playInObs')"
                  :icon="testing === row.id ? 'pi pi-stop' : 'pi pi-play'"
                  text
                  :aria-describedby="`reward-name-${row.id}`"
                  :disabled="testing !== row.id && !canTest(row.id)"
                  @click="testing === row.id ? stopSound() : testRow(row.id)"
                />
              </td>
            </template>
          </tr>
        </tbody>
      </table>
      <div class="tbl-foot">
        <span class="muted">{{ t('rewardPlayback.filesHint') }}</span>
      </div>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputNumber from 'primevue/inputnumber'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { useModules } from './features'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'
import ObsStatusLink from './ObsStatusLink.vue'

type Position = Schemas['MediaPosition']
type Rule = Schemas['MediaRuleBody']
const DEFAULT = 'default'
const POSITIONS: Position[] = ['top_left', 'top_right', 'bottom_left', 'bottom_right', 'center', 'full']

const { t } = useI18n()
const toast = useToast()
const { hidden } = useModules()
const showSounds = computed(() => !hidden.value.has('/sounds'))
const showMedia = computed(() => !hidden.value.has('/media'))
const controlCols = computed(() => (showSounds.value ? 1 : 0) + (showMedia.value ? 3 : 0))

const library = shallowRef<Schemas['SoundLibrary']>()
const media = shallowRef<Schemas['MediaItem'][]>([])
const rules = shallowRef<Rule[]>([])
const rewards = shallowRef<Schemas['Reward'][]>()
const checkinRewardId = ref<string | null>(null)
const rewardError = ref('')

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))
const report = (e: unknown) => toast.add(errorToast(message(e)))

const loading = computed(() => !rewards.value && !rows.value.length)

const soundOptions = computed(() => [
  { value: DEFAULT, label: t('sound.silent') },
  ...(library.value?.sounds ?? []).map((s) => ({ value: s.id, label: s.label })),
])

const isSoundFile = (v: string) => v !== DEFAULT && v !== 'silent'

const soundOf = (rewardId: string) => {
  const chosen = library.value?.slots[`reward.${rewardId}`] ?? DEFAULT
  return chosen === 'silent' ? DEFAULT : chosen
}

const ruleOf = (rewardId: string) => rules.value.find((r) => r.reward_id === rewardId)
const itemOf = (rewardId: string) => media.value.find((m) => m.id === ruleOf(rewardId)?.media_id)
const isGif = (rewardId: string) => itemOf(rewardId)?.format === 'gif'

// Channel rewards, plus any reward IDs with a saved sound or media rule that Twitch no longer lists (shown by ID).
const rows = computed(() => {
  const titles = new Map((rewards.value ?? []).map((r) => [r.id, r.title]))
  const ids = [
    ...(rewards.value ?? []).map((r) => r.id),
    ...(showSounds.value
      ? Object.keys(library.value?.slots ?? {}).flatMap((s) => (s.startsWith('reward.') ? [s.slice(7)] : []))
      : []),
    ...(showMedia.value ? rules.value.map((r) => r.reward_id) : []),
  ]
  return [...new Set(ids)].map((id) => ({
    id,
    title: titles.get(id) ?? id,
    isCheckin: Boolean(checkinRewardId.value && id === checkinRewardId.value),
  }))
})

const canTest = (rewardId: string) =>
  (showSounds.value && isSoundFile(soundOf(rewardId))) || (showMedia.value && Boolean(ruleOf(rewardId)))

// Serialize media rule PUT/DELETE calls so each edit builds from the latest saved state.
let ruleQueue: Promise<unknown> = Promise.resolve()
function serialRule(task: () => Promise<void>) {
  ruleQueue = ruleQueue.then(task).catch(report)
  return ruleQueue
}

async function setSound(rewardId: string, value: string) {
  const slot = `reward.${rewardId}`
  try {
    await api(client.PUT('/api/admin/sounds/slots/{slot}', { params: { path: { slot } }, body: { sound: value === DEFAULT ? null : value } }))
    if (library.value) {
      const slots = { ...library.value.slots }
      if (value === DEFAULT) delete slots[slot]
      else slots[slot] = value
      library.value = { ...library.value, slots }
    }
  } catch (e) {
    report(e)
  }
}

const saveRule = (rewardId: string, patch: Partial<Schemas['MediaRuleInput']>) =>
  serialRule(async () => {
    const rule = ruleOf(rewardId)
    if (!rule) return
    await api(
      client.PUT('/api/admin/screen-tools/media-rules/{reward_id}', {
        params: { path: { reward_id: rewardId } },
        body: { media_id: rule.media_id, position: rule.position, volume: rule.volume ?? null, ...patch },
      }),
    )
    rules.value = await api(client.GET('/api/admin/screen-tools/media-rules'))
  })

const setMedia = (rewardId: string, mediaId: string | null) =>
  serialRule(async () => {
    if (mediaId === null) {
      await api(client.DELETE('/api/admin/screen-tools/media-rules/{reward_id}', { params: { path: { reward_id: rewardId } } }))
    } else {
      const rule = ruleOf(rewardId)
      await api(
        client.PUT('/api/admin/screen-tools/media-rules/{reward_id}', {
          params: { path: { reward_id: rewardId } },
          body: { media_id: mediaId, position: rule?.position ?? 'bottom_right', volume: rule?.volume ?? null },
        }),
      )
    }
    rules.value = await api(client.GET('/api/admin/screen-tools/media-rules'))
  })

const testing = ref<string>()
let ended: ReturnType<typeof setTimeout> | undefined
function testOver() {
  clearTimeout(ended)
  testing.value = undefined
}
onBeforeUnmount(() => clearTimeout(ended))

async function testRow(rewardId: string) {
  const soundId = showSounds.value ? soundOf(rewardId) : DEFAULT
  const hasSound = isSoundFile(soundId)
  const hasMedia = showMedia.value && Boolean(ruleOf(rewardId))
  try {
    if (hasSound) {
      await api(client.POST('/api/admin/sounds/{id}/play', { params: { path: { id: soundId } } }))
      testOver()
      testing.value = rewardId
      const ms = library.value?.sounds.find((s) => s.id === soundId)?.duration_ms
      if (ms) ended = setTimeout(testOver, ms)
    }
    if (hasMedia) {
      await api(client.POST('/api/admin/screen-tools/media-rules/{reward_id}/test', { params: { path: { reward_id: rewardId } } }))
    }
    if (hasSound || hasMedia) toast.add(successToast(t('sound.playedInObs')))
  } catch (e) {
    report(e)
  }
}

async function stopSound() {
  try {
    await api(client.POST('/api/admin/sounds/stop'))
    testOver()
  } catch (e) {
    report(e)
  }
}

void api(client.GET('/api/admin/checkin/rewards')).then(
  (r) => (rewards.value = r),
  (e: unknown) => (rewardError.value = message(e)),
)
void api(client.GET('/api/admin/checkin/settings')).then(
  (s) => (checkinRewardId.value = s.reward_id ?? null),
  () => {},
)
if (showSounds.value) void api(client.GET('/api/admin/sounds')).then((l) => (library.value = l), report)
if (showMedia.value) {
  void api(client.GET('/api/admin/screen-tools/media')).then((m) => (media.value = m), report)
  void api(client.GET('/api/admin/screen-tools/media-rules')).then((r) => (rules.value = r), report)
}
</script>

<style scoped>
.sources {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1.5rem;
  align-items: center;
}

.sources span {
  display: inline-flex;
  gap: 0.5rem;
  align-items: center;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.checkin-note {
  font-weight: 400;
  font-size: 0.75rem;
}

.playback-table :deep(.p-select) {
  min-width: 0;
  max-width: 14rem;
}

.volume {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

.volume :deep(.p-inputnumber-input) {
  width: 5rem;
}

.end {
  text-align: right;
  white-space: nowrap;
}
</style>
