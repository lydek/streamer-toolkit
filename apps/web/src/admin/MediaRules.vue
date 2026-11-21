<template>
  <div class="stack">
    <p v-if="rewardError" role="alert">{{ rewardError }}</p>
    <p v-else-if="!rewards">{{ t('app.loading') }}</p>
    <p v-else-if="!rewards.length">{{ t('screenTools.rules.noRewards') }}</p>
    <table v-else class="table">
      <thead>
        <tr>
          <th scope="col">{{ t('screenTools.rules.reward') }}</th>
          <th scope="col">{{ t('screenTools.rules.media') }}</th>
          <th scope="col">{{ t('screenTools.media.length') }}</th>
          <th scope="col">{{ t('screenTools.rules.position') }}</th>
          <th scope="col">{{ t('screenTools.rules.volume') }}</th>
          <th scope="col"><span class="sr-only">{{ t('screenTools.media.actions') }}</span></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in rewards" :key="r.id" :data-reward="r.id">
          <th scope="row">{{ r.title }}</th>
          <td>
            <Select
              :model-value="ruleOf(r.id)?.media_id ?? null"
              :options="media"
              option-label="title"
              option-value="id"
              :placeholder="t('screenTools.rules.none')"
              :aria-label="t('screenTools.rules.mediaFor', { reward: r.title })"
              show-clear
              @update:model-value="(id: string | null) => setMedia(r.id, id)"
            />
          </td>
          <td>{{ lengthOf(r.id) }}</td>
          <td>
            <Select
              :model-value="ruleOf(r.id)?.position ?? 'bottom_right'"
              :options="POSITIONS"
              :option-label="(p: Position) => t(`screenTools.rules.positions.${p}`)"
              :aria-label="t('screenTools.rules.positionFor', { reward: r.title })"
              :disabled="!ruleOf(r.id)"
              @update:model-value="(position: Position) => save(r.id, { position })"
            />
          </td>
          <td>
            <span class="volume">
              <Checkbox
                :model-value="ruleOf(r.id)?.volume == null"
                :input-id="`follow-${r.id}`"
                binary
                :disabled="!ruleOf(r.id) || isGif(r.id)"
                @update:model-value="(follow: boolean) => save(r.id, { volume: follow ? null : 100 })"
              />
              <label :for="`follow-${r.id}`">{{ t('screenTools.rules.followMaster') }}</label>
              <InputNumber
                v-if="ruleOf(r.id) && ruleOf(r.id)!.volume != null"
                :model-value="ruleOf(r.id)!.volume"
                :min="0"
                :max="100"
                suffix="%"
                :aria-label="t('screenTools.rules.volumeFor', { reward: r.title })"
                @update:model-value="(v: number | null) => v !== null && save(r.id, { volume: v })"
              />
            </span>
          </td>
          <td class="end">
            <Button :label="t('screenTools.rules.test')" text :disabled="!ruleOf(r.id)" @click="test(r.id)" />
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputNumber from 'primevue/inputnumber'
import Select from 'primevue/select'
import { ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { formatLength } from './music'

type Position = Schemas['MediaPosition']
type Rule = Schemas['MediaRuleBody']
const POSITIONS: Position[] = ['top_left', 'top_right', 'bottom_left', 'bottom_right', 'center', 'full']

const props = defineProps<{
  /** The media library, for the file choices; the parent reloads it after an import. */
  media: Schemas['MediaItem'][]
}>()
const emit = defineEmits<{ error: [unknown] }>()
const { t } = useI18n()
const rewards = shallowRef<Schemas['Reward'][]>()
const rewardError = ref('')
const rules = shallowRef<Rule[]>([])

const ruleOf = (reward: string) => rules.value.find((r) => r.reward_id === reward)
const itemOf = (reward: string) => props.media.find((m) => m.id === ruleOf(reward)?.media_id)
const lengthOf = (reward: string) => {
  const ms = itemOf(reward)?.duration_ms
  return ms == null ? '' : formatLength(ms)
}
// A GIF has no sound.
const isGif = (reward: string) => itemOf(reward)?.format === 'gif'

async function load() {
  try {
    rewards.value = await api(client.GET('/api/admin/checkin/rewards'))
    rewardError.value = ''
  } catch (e) {
    rewardError.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
  await loadRules()
}

async function loadRules() {
  rules.value = await api(client.GET('/api/admin/screen-tools/media-rules')).catch((e: unknown) => {
    emit('error', e)
    return []
  })
}

// Each PUT sends the whole rule, so changes run one at a time, each built from the rules the previous one reloaded.
let queue: Promise<unknown> = Promise.resolve()
function serial(task: () => Promise<void>) {
  queue = queue.then(task).catch((e: unknown) => emit('error', e))
  return queue
}

const save = (reward: string, patch: Partial<Schemas['MediaRuleInput']>) => serial(() => saveNow(reward, patch))
const setMedia = (reward: string, mediaId: string | null) => serial(() => setMediaNow(reward, mediaId))

async function saveNow(reward: string, patch: Partial<Schemas['MediaRuleInput']>) {
  const rule = ruleOf(reward)
  if (!rule) return
  try {
    await api(
      client.PUT('/api/admin/screen-tools/media-rules/{reward_id}', {
        params: { path: { reward_id: reward } },
        body: { media_id: rule.media_id, position: rule.position, volume: rule.volume ?? null, ...patch },
      }),
    )
    rules.value = await api(client.GET('/api/admin/screen-tools/media-rules'))
  } catch (e) {
    emit('error', e)
  }
}

async function setMediaNow(reward: string, mediaId: string | null) {
  try {
    if (mediaId === null) {
      await api(client.DELETE('/api/admin/screen-tools/media-rules/{reward_id}', { params: { path: { reward_id: reward } } }))
    } else {
      const rule = ruleOf(reward)
      await api(
        client.PUT('/api/admin/screen-tools/media-rules/{reward_id}', {
          params: { path: { reward_id: reward } },
          body: { media_id: mediaId, position: rule?.position ?? 'bottom_right', volume: rule?.volume ?? null },
        }),
      )
    }
    rules.value = await api(client.GET('/api/admin/screen-tools/media-rules'))
  } catch (e) {
    emit('error', e)
  }
}

async function test(reward: string) {
  try {
    await api(client.POST('/api/admin/screen-tools/media-rules/{reward_id}/test', { params: { path: { reward_id: reward } } }))
  } catch (e) {
    emit('error', e)
  }
}

void load()
// Deleting a file in the library deletes its rules on the Host too.
watch(
  () => props.media,
  () => void serial(loadRules),
)
</script>

<style scoped>
.table {
  width: 100%;
  border-collapse: collapse;
}

.table th,
.table td {
  padding: 0.5rem;
  text-align: left;
  border-bottom: 1px solid var(--p-content-border-color);
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
}
</style>
