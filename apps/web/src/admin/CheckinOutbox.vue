<template>
  <div class="stack">
    <template v-if="state && !state.enabled">
      <p v-if="state.closed">{{ t('checkin.outbox.closed') }}</p>
      <template v-else>
        <p>
          {{ t('checkin.outbox.paused', { n: outbox?.pending ?? 0 }) }}
          <template v-if="outbox?.oldest_pending_at">
            {{ t('checkin.outbox.oldest') }}<time :datetime="outbox.oldest_pending_at">{{ formatTime(outbox.oldest_pending_at) }}</time>
          </template>
        </p>
        <Button :label="t('checkin.outbox.switchToClose')" outlined :disabled="busy" @click="emit('close', outbox?.pending ?? 0)" />
      </template>
    </template>
    <template v-if="outbox?.dead_letters.length">
      <h3 id="checkin-dead-letters">{{ t('checkin.outbox.deadLetters') }}</h3>
      <ul class="plain" aria-labelledby="checkin-dead-letters">
        <li v-for="d in outbox.dead_letters" :key="d.event_id" class="dead">
          <span>
            <time :datetime="d.occurred_at">{{ formatTime(d.occurred_at) }}</time>
            {{ d.user_name ?? '' }} {{ d.reward_title ? `「${d.reward_title}」` : '' }}
          </span>
          <span class="error">{{ d.error }}</span>
          <span class="buttons">
            <Button :label="t('checkin.outbox.retry')" size="small" :disabled="busy" @click="act(d.event_id, 'retry')" />
            <Button :label="t('checkin.outbox.skip')" size="small" outlined :disabled="busy" @click="act(d.event_id, 'skip')" />
          </span>
        </li>
      </ul>
    </template>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'
import { loadOutbox, useCheckinOutbox } from './checkinOutbox'

// Backlog while paused and redemptions that kept failing; viewer and reward text is shown as text only.
defineProps<{ state: Schemas['ModuleState'] | undefined }>()
const emit = defineEmits<{ close: [pending: number]; error: [unknown] }>()
const { t, locale } = useI18n()
// Redemptions keep arriving while paused, and retried events settle in the background: the shared poll follows them.
const { outbox } = useCheckinOutbox()
const busy = ref(false)

const formatTime = (iso: string) =>
  new Date(iso).toLocaleString(locale.value, { dateStyle: 'short', timeStyle: 'short' })

// A fresh read: one already in flight may predate the change just made.
const load = () => loadOutbox(true)

async function act(eventId: string, action: 'retry' | 'skip') {
  busy.value = true
  try {
    const params = { params: { path: { event_id: eventId } } }
    await api(
      action === 'retry'
        ? client.POST('/api/admin/checkin/outbox/{event_id}/retry', params)
        : client.POST('/api/admin/checkin/outbox/{event_id}/skip', params),
    )
    await load()
  } catch (e) {
    emit('error', e)
  } finally {
    busy.value = false
  }
}

onMounted(() => load().catch((e) => emit('error', e)))

defineExpose({ load })
</script>

<style scoped>
.plain {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.dead {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.error {
  overflow-wrap: anywhere;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

h3,
p {
  margin: 0;
}
</style>
