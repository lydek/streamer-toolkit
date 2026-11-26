<template>
  <p v-if="!records.length">{{ empty }}</p>
  <!-- Viewer and Twitch text only ever goes in as text ({{ }}), never as markup. -->
  <ul v-else class="records">
    <li v-for="r in records" :key="r.id" class="record">
      <div class="head">
        <span class="name">{{ r.user_name }}</span>
        <span>{{ r.reward_title }}</span>
        <span class="cost">{{ t('redemption.cost', { n: r.reward_cost }) }}</span>
        <!-- Icon and text, so the status never depends on color alone. -->
        <span class="status" :data-status="r.status">
          <i :class="ICONS[r.status]" aria-hidden="true" />{{ t(`redemption.status.${r.status}`) }}
        </span>
      </div>
      <p v-if="r.user_input" class="input">{{ r.user_input }}</p>
      <!-- What it played: — for nothing, 沒有紀錄 for a record from before this was kept. -->
      <p class="played">{{ t('redemption.played.label') }}{{ playedText(r) }}</p>
      <div class="foot">
        <time :datetime="r.redeemed_at">{{ formatTime(r.redeemed_at) }}</time>
        <!-- Waiting redemptions of the toolkit's own rewards can be handled here; Twitch allows no others. -->
        <span v-if="r.status === 'unfulfilled' && own" class="actions">
          <template v-if="own.has(r.reward_id)">
            <template v-if="canManage">
              <Button :label="t('redemption.fulfil')" size="small" outlined :disabled="busy === r.twitch_id" @click="set(r, 'fulfilled')" />
              <Button :label="t('redemption.refund')" size="small" text :disabled="busy === r.twitch_id" @click="askRefund(r)" />
            </template>
            <Button v-else :label="t('redemption.grant')" icon="pi pi-lock" size="small" text @click="grant" />
          </template>
          <i
            v-else
            class="pi pi-info-circle not-ours"
            tabindex="0"
            role="img"
            :title="t('redemption.notOurs')"
            :aria-label="t('redemption.notOurs')"
          />
        </span>
      </div>
    </li>
  </ul>
  <dialog v-if="records.length" ref="refundDialog" class="sheet" aria-labelledby="refund-title" @close="onRefundClose">
    <h2 id="refund-title">{{ t('redemption.refundTitle', { name: refunding?.user_name, reward: refunding?.reward_title }) }}</h2>
    <p>{{ t('redemption.refundText', { n: refunding?.reward_cost }) }}</p>
    <div class="buttons sheet-actions">
      <Button :label="t('redemption.refund')" severity="danger" @click="refundDialog?.close('ok')" />
      <Button :label="t('features.cancel')" text autofocus @click="refundDialog?.close()" />
    </div>
  </dialog>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { errorMessage } from '../i18n'
import { ApiError, api, client } from './api'
import { errorToast, successToast, twitchOverview } from './home'
import type { RedemptionRecord, RedemptionStatus } from './redemptions'
import { requestScopes } from './scopes'

const SCOPE = 'channel:manage:redemptions'

const props = defineProps<{ records: RedemptionRecord[]; empty: string }>()
const { t, locale } = useI18n()
const toast = useToast()
const router = useRouter()

// The toolkit's own rewards, read once a waiting redemption is shown while signed in; a failed read shows no
// buttons and is tried again at the next sign-in. Signing out forgets them, so another account's never show.
const own = ref<Set<string>>()
let reading = false
watch(
  [() => props.records.some((r) => r.status === 'unfulfilled'), () => twitchOverview.value?.status.state],
  ([waiting, state]) => {
    if (state !== 'signed_in') {
      own.value = undefined
      return
    }
    if (!waiting || own.value || reading) return
    reading = true
    api(client.GET('/api/admin/rewards'))
      .then((list) => (own.value = new Set(list.filter((r) => r.manageable).map((r) => r.id))))
      .catch(() => {})
      .finally(() => (reading = false))
  },
  { immediate: true },
)
const canManage = computed(() => !!twitchOverview.value?.scopes?.includes(SCOPE))
const busy = ref<string>()
const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))

/** The page and drawer reload on the `redemption.changed` that follows. */
async function set(r: RedemptionRecord, status: 'fulfilled' | 'canceled') {
  busy.value = r.twitch_id
  try {
    await api(
      client.PUT('/api/admin/redemptions/{twitch_id}/status', {
        params: { path: { twitch_id: r.twitch_id } },
        body: { reward_id: r.reward_id, status },
      }),
    )
    toast.add(successToast(t(status === 'fulfilled' ? 'redemption.fulfilled' : 'redemption.refunded', { name: r.user_name })))
  } catch (e) {
    toast.add(errorToast(message(e)))
  } finally {
    busy.value = undefined
  }
}

// Refunding gives the viewer the points back, so it is confirmed first.
const refundDialog = ref<HTMLDialogElement>()
const refunding = ref<RedemptionRecord>()
function askRefund(r: RedemptionRecord) {
  refunding.value = r
  refundDialog.value!.returnValue = ''
  refundDialog.value!.showModal()
}
function onRefundClose() {
  const r = refunding.value
  if (r && refundDialog.value?.returnValue === 'ok') void set(r, 'canceled')
  refunding.value = undefined
}

async function grant() {
  try {
    await requestScopes([SCOPE], router)
  } catch (e) {
    toast.add(errorToast(message(e)))
  }
}

const ICONS: Record<RedemptionStatus, string> = {
  unfulfilled: 'pi pi-clock',
  fulfilled: 'pi pi-check',
  canceled: 'pi pi-replay',
  unknown: 'pi pi-question-circle',
}

function playedText(r: RedemptionRecord) {
  if (!r.played) return t('redemption.played.untracked')
  if (!r.played.length) return t('redemption.played.none')
  return r.played
    .map((p) => t('redemption.played.entry', { what: t(`redemption.played.what.${p.what}`), result: t(`redemption.played.result.${p.result}`) }))
    .join('、')
}

const formatTime = (iso: string) =>
  new Date(iso).toLocaleString(locale.value, { dateStyle: 'short', timeStyle: 'short' })
</script>

<style scoped>
.records {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.record {
  padding: 0.5rem 0.75rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
}

.head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 0.25rem 0.75rem;
}

.name {
  font-weight: 600;
}

.status {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  margin-inline-start: auto;
}

.input {
  margin: 0.25rem 0;
  overflow-wrap: anywhere;
}

.name,
.head span {
  overflow-wrap: anywhere;
}

time {
  font-size: 0.875rem;
}

.foot {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.375rem 0.75rem;
}

.actions {
  display: inline-flex;
  gap: 0.25rem;
  margin-inline-start: auto;
}

.not-ours {
  color: var(--p-text-muted-color);
}

.played {
  margin: 0.25rem 0 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
