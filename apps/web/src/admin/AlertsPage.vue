<template>
  <FullPage :title="t('modules.names.alerts')">
    <TemplatePicker overlay="alerts" :ids="BUILT_IN" :label="(id) => t(`alerts.templates.${id}`)" :start="start" />
    <section aria-labelledby="alert-sounds">
      <h2 id="alert-sounds">{{ t('modules.names.sound') }}</h2>
      <SoundSlots built-in :slots="ALERT_KINDS.map((k) => ({ slot: `alert.${k}`, label: t(`alerts.kinds.${k}`) }))" />
    </section>
    <RecentAlerts />
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { ALERT_KINDS } from '../overlay/alerts/alerts'
import type { Incoming } from '../overlay/chat/chat'
import type { Schemas } from './api'
import FullPage from './FullPage.vue'
import RecentAlerts from './RecentAlerts.vue'
import SoundSlots from './SoundSlots.vue'
import TemplatePicker from './TemplatePicker.vue'

// Keys of a full record, so a template added to the Host's list fails the typecheck until it is listed here.
const BUILT_IN = Object.keys({ default: 0, banner: 0, text: 0 } satisfies Record<Schemas['AlertTemplate'], 0>)

const { t } = useI18n()

/** A sample resub with a message, which a preview plays over and over; live alerts take its place. */
const start = (): Incoming[] => [
  { type: 'snapshot', version: 1, seq: 0, payload: { 'chat.removed': [], 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [] } },
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
      sound: { kind: 'built_in' },
    },
  },
]
</script>
