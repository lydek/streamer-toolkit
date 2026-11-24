<template>
  <FullPage tile="alerts" overlay="alerts">
    <!-- 樣板: the first of the alerts pages (每種事件 and 最近事件 are their own pages). -->
    <TemplatePicker overlay="alerts" :ids="BUILT_IN" :label="(id) => t(`alerts.templates.${id}`)" :start="start" />
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { Incoming } from '../overlay/chat/chat'
import type { Schemas } from './api'
import FullPage from './FullPage.vue'
import TemplatePicker from './TemplatePicker.vue'

// Keys of a full record, so a template added to the Host's list fails the typecheck until it is listed here.
const BUILT_IN = Object.keys({ default: 0, banner: 0, text: 0 } satisfies Record<Schemas['AlertTemplate'], 0>)

const { t } = useI18n()

/** A sample resub with a message, which a preview plays over and over; live alerts take its place. */
const start = (): Incoming[] => [
  { type: 'snapshot', version: 1, seq: 0, payload: { 'chat.removed': [], 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'chat.text_size': [] } },
  {
    id: 'sample',
    type: 'alert',
    version: 1,
    seq: 0,
    occurredAt: new Date(0).toISOString(),
    source: 'simulation',
    payload: {
      kind: 'resubscribe',
      userId: 'sample',
      userLogin: 'viewer',
      userName: 'Viewer',
      tier: 1,
      months: 12,
      message: t('alerts.sample'),
      durationMs: 4000,
      sound: { kind: 'built_in', volume: 100 },
    },
  },
]
</script>
