<template>
  <!-- 頻道點數 › 獎勵: the channel point rewards on Twitch. The toolkit's own can be switched; those made in the Twitch
       dashboard are read-only here (Twitch lets an app change only the rewards it made). -->
  <FullPage :status="status">
    <div class="rewards">
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
      <section class="tbl-wrap" aria-labelledby="rw-heading">
        <div class="tbl-top">
          <div>
            <h3 id="rw-heading" class="tbl-head">{{ t('rewards.heading') }}</h3>
            <span v-if="rewards" class="sub">{{ t('rewards.count', { count: rewards.length, max: MAX_REWARDS, own: own.length }) }}</span>
          </div>
          <span v-if="full" class="sub">{{ t('rewards.full', { max: MAX_REWARDS }) }}</span>
          <Button :label="t('rewards.add')" icon="pi pi-plus" :disabled="!rewards || full" @click="edit()" />
        </div>
        <p v-if="!rewards && !loadFailed" class="empty-cell">{{ t('app.loading') }}</p>
        <p v-else-if="rewards && !rewards.length" class="empty-cell">{{ t('rewards.empty') }}</p>
        <table v-else-if="rewards" class="table">
          <thead>
            <tr>
              <th scope="col">{{ t('rewards.reward') }}</th>
              <th scope="col" class="num">{{ t('rewards.cost') }}</th>
              <th scope="col">{{ t('rewards.cooldown') }}</th>
              <th scope="col">{{ t('rewards.usedBy') }}</th>
              <th scope="col" class="num">{{ t('rewards.enabled') }}</th>
              <th scope="col" class="num">{{ t('rewards.paused') }}</th>
              <th scope="col"><span class="sr-only">{{ t('rewards.actions') }}</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in rewards" :key="r.id" :data-reward="r.id" :class="{ ro: !r.manageable }">
              <th scope="row">
                <span class="name">
                  <span class="swatch" :style="{ background: r.background_color }" aria-hidden="true"></span>
                  <span>
                    <b>{{ r.title }}</b>
                    <small>{{ detail(r) }}</small>
                  </span>
                </span>
              </th>
              <td class="num">{{ n(r.cost) }}</td>
              <td>{{ cooldown(r.cooldown_seconds) }}</td>
              <td>
                <span v-if="usedBy(r.id).length" class="used">
                  <span v-for="u in usedBy(r.id)" :key="u">{{ u }}</span>
                </span>
                <span v-else class="muted">{{ t('rewards.unused') }}</span>
              </td>
              <template v-if="r.manageable">
                <td class="num">
                  <ToggleSwitch
                    :model-value="r.is_enabled"
                    :disabled="busy === r.id"
                    :aria-label="t('rewards.enableLabel', { name: r.title })"
                    @update:model-value="change(r, { is_enabled: $event })"
                  />
                </td>
                <td class="num">
                  <ToggleSwitch
                    :model-value="r.is_paused"
                    :disabled="busy === r.id || !r.is_enabled"
                    :aria-label="t('rewards.pauseLabel', { name: r.title })"
                    @update:model-value="change(r, { is_paused: $event })"
                  />
                </td>
                <td class="num">
                  <Button icon="pi pi-pencil" text :aria-label="t('rewards.editLabel', { name: r.title })" @click="edit(r)" />
                </td>
              </template>
              <template v-else>
                <td class="num muted" colspan="2">{{ t('rewards.dashboardOnly') }}</td>
                <td class="num">
                  <Button :label="t('rewards.takeover.button')" text :disabled="full" @click="takeover?.open(r, titles)" />
                </td>
              </template>
            </tr>
          </tbody>
        </table>
      </section>
      <p class="note">{{ t('rewards.note') }}</p>
    </div>
    <RewardDialog ref="dialog" @saved="reload" />
    <TakeoverDialog ref="takeover" @done="reloadAll" />
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast } from './home'
import RewardDialog from './RewardDialog.vue'
import TakeoverDialog from './TakeoverDialog.vue'

type Reward = Schemas['ChannelReward']
// Twitch's limit of custom rewards per channel.
const MAX_REWARDS = 50

const { t, n } = useI18n()
const toast = useToast()
const rewards = shallowRef<Reward[]>()
const loadFailed = ref('')
const busy = ref<string>()
// What uses each reward: check-in, a reward sound, a reward video or GIF.
const checkinReward = ref<string | null>(null)
const library = shallowRef<Schemas['SoundLibrary']>()
const media = shallowRef<Schemas['MediaItem'][]>([])
const rules = shallowRef<Schemas['MediaRuleBody'][]>([])

const own = computed(() => rewards.value?.filter((r) => r.manageable) ?? [])
const full = computed(() => (rewards.value?.length ?? 0) >= MAX_REWARDS)
const dialog = ref<InstanceType<typeof RewardDialog>>()
const takeover = ref<InstanceType<typeof TakeoverDialog>>()
const titles = computed(() => rewards.value?.map((x) => x.title) ?? [])
const status = computed(() => {
  if (!rewards.value) return ''
  const on = rewards.value.filter((r) => r.is_enabled)
  return t('rewards.summary', { on: on.length, paused: on.filter((r) => r.is_paused).length })
})

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))

function cooldown(seconds?: number | null) {
  if (!seconds) return '—'
  return seconds % 60 ? t('rewards.seconds', { n: seconds }) : t('rewards.minutes', { n: seconds / 60 })
}

function detail(r: Reward) {
  if (!r.manageable) return t('rewards.readOnly')
  const parts = [t(r.is_user_input_required ? 'rewards.inputYes' : 'rewards.inputNo')]
  if (r.max_per_stream) parts.push(t('rewards.perStream', { n: r.max_per_stream }))
  if (r.max_per_user_per_stream) parts.push(t('rewards.perUser', { n: r.max_per_user_per_stream }))
  return parts.join(' · ')
}

function usedBy(id: string) {
  const uses: string[] = []
  if (checkinReward.value === id) uses.push(t('rewards.used.checkin'))
  const sound = library.value?.slots[`reward.${id}`]
  const soundName = library.value?.sounds.find((s) => s.id === sound)?.label
  if (soundName) uses.push(t('rewards.used.sound', { name: soundName }))
  const rule = rules.value.find((r) => r.reward_id === id)
  const mediaName = media.value.find((m) => m.id === rule?.media_id)?.title
  if (mediaName) uses.push(t('rewards.used.media', { name: mediaName }))
  return uses
}

async function load() {
  rewards.value = await api(client.GET('/api/admin/rewards'))
}

function edit(r?: Reward) {
  dialog.value?.open(r, titles.value, r ? usedBy(r.id) : [])
}

function reload() {
  load().catch((e: unknown) => toast.add(errorToast(message(e))))
}

async function change(r: Reward, body: { is_enabled?: boolean; is_paused?: boolean }) {
  busy.value = r.id
  try {
    await api(client.PATCH('/api/admin/rewards/{id}', { params: { path: { id: r.id } }, body }))
    await load()
  } catch (e) {
    toast.add(errorToast(message(e)))
  } finally {
    busy.value = undefined
  }
}

// Each is only for the "used by" column; one failing leaves its part out.
function loadUses() {
  void api(client.GET('/api/admin/checkin/settings')).then((s) => (checkinReward.value = s.reward_id ?? null), () => {})
  void api(client.GET('/api/admin/sounds')).then((l) => (library.value = l), () => {})
  void api(client.GET('/api/admin/screen-tools/media')).then((m) => (media.value = m), () => {})
  void api(client.GET('/api/admin/screen-tools/media-rules')).then((r) => (rules.value = r), () => {})
}

// A takeover also moves what used the original.
function reloadAll() {
  reload()
  loadUses()
}

load().catch((e: unknown) => (loadFailed.value = message(e)))
loadUses()
</script>

<style scoped>
.rewards {
  max-width: 72rem;
}

.sub,
.muted,
small {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.name {
  display: inline-flex;
  gap: 0.625rem;
  align-items: center;
  font-weight: 400;
}

.name > span:last-child {
  display: grid;
}

.swatch {
  flex: none;
  width: 0.875rem;
  height: 0.875rem;
  border-radius: 0.25rem;
}

.used {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 0.25rem;
}

.used span {
  padding: 0.0625rem 0.5rem;
  border-radius: 999px;
  background: var(--p-surface-800);
  font-size: 0.8125rem;
}

.num {
  text-align: end;
}

.note {
  margin: 0.75rem 0 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
