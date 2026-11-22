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
      <time :datetime="r.redeemed_at">{{ formatTime(r.redeemed_at) }}</time>
    </li>
  </ul>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { RedemptionRecord, RedemptionStatus } from './redemptions'

defineProps<{ records: RedemptionRecord[]; empty: string }>()
const { t, locale } = useI18n()

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

.played {
  margin: 0.25rem 0 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
