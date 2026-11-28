<template>
  <!-- 最近事件 on the alerts page: what became of each alert this launch, and 重播 with the current settings.
       `compact` (the alerts drawer): the latest five under a smaller heading. -->
  <section :aria-labelledby="headingId" :class="{ compact }">
    <component :is="compact ? 'h3' : 'h2'" :id="headingId">{{ t('alerts.recent.title') }}</component>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="recent && !recent.length" class="muted">{{ t('alerts.recent.empty') }}</p>
    <!-- A table in a card, as the prototype; one outcome per alert, as the Host records it. -->
    <div v-else-if="recent" class="tbl-wrap">
      <table class="table recent">
        <thead>
          <tr>
            <th scope="col">{{ t('alerts.recent.cols.when') }}</th>
            <th scope="col">{{ t('alerts.recent.cols.what') }}</th>
            <th scope="col">{{ t('alerts.recent.cols.outcome') }}</th>
            <th scope="col"><span class="sr-only">{{ t('alerts.recent.replay') }}</span></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in compact ? recent.slice(0, COMPACT_ROWS) : recent" :key="r.id">
            <td><time :datetime="r.occurredAt">{{ clock(r.occurredAt) }}</time></td>
            <td class="what">
              {{ alertTitle(r.payload) }}
              <span v-if="r.source === 'simulation'" class="tag">{{ t('alerts.recent.simulated') }}</span>
            </td>
            <td>
              <span class="outcome" :class="look(r.outcome).kind">
                <i :class="look(r.outcome).icon" aria-hidden="true"></i>{{ t(`alerts.recent.outcome.${r.outcome ?? 'pending'}`) }}
              </span>
            </td>
            <td class="end">
              <Button :label="t('alerts.recent.replay')" icon="pi pi-replay" text size="small" :disabled="busy === r.id" @click="replay(r.id)" />
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { alertTitle } from '../overlay/alerts/alerts'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast, successToast } from './home'

// Outcomes change as the queue plays; read the list again this often while the page is open.
const POLL_MS = 2000
const COMPACT_ROWS = 5

/** `notify`: where to say a replay's result (the drawer's notice; a toast without it). */
const props = defineProps<{ compact?: boolean; notify?: (text: string, error: boolean) => void }>()
// Its own id in the drawer, so it never clashes with the page's.
const headingId = props.compact ? 'recent-alerts-drawer' : 'recent-alerts'

const { t } = useI18n()
const toast = useToast()
const recent = shallowRef<Schemas['RecentAlertBody'][]>()
const failed = ref('')
const busy = ref<string>()

async function load() {
  try {
    recent.value = await api(client.GET('/api/admin/alerts/recent'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}
void load()
const timer = window.setInterval(() => void load(), POLL_MS)
onBeforeUnmount(() => window.clearInterval(timer))

const two = (n: number) => String(n).padStart(2, '0')
const clock = (at: string) => {
  const d = new Date(at)
  return `${two(d.getHours())}:${two(d.getMinutes())}`
}
/** The outcome tag's look: played (green), waiting, a problem the streamer can fix (the sound file missing), or not
 *  played on purpose or by circumstance (grey). The words always say which; the colour is never the only cue. */
function look(o: Schemas['Outcome'] | null): { kind: string; icon: string } {
  if (o === null) return { kind: 'wait', icon: 'pi pi-clock' }
  if (o === 'played') return { kind: 'ok', icon: 'pi pi-check' }
  if (o === 'sound_missing') return { kind: 'bad', icon: 'pi pi-exclamation-triangle' }
  return { kind: 'skip', icon: 'pi pi-minus' }
}

const say = (text: string, error: boolean) =>
  props.notify ? props.notify(text, error) : toast.add(error ? errorToast(text) : successToast(text))

async function replay(id: string) {
  busy.value = id
  try {
    await api(client.POST('/api/admin/alerts/recent/{id}/replay', { params: { path: { id } } }))
    say(t('alerts.recent.replayed'), false)
    await load()
  } catch (e) {
    say(e instanceof ApiError ? errorMessage(e.code) : String(e), true)
  } finally {
    busy.value = undefined
  }
}
</script>

<style scoped>
.compact h3 {
  margin: 0 0 0.375rem;
  font-size: 0.9375rem;
}

.compact .tbl-wrap {
  margin-top: 0;
}

.muted,
time {
  color: var(--p-text-muted-color);
}

.recent .end {
  text-align: end;
}

time {
  font-variant-numeric: tabular-nums;
}

.what {
  overflow-wrap: anywhere;
}

.tag {
  margin-left: 0.25rem;
  padding: 0 0.375rem;
  border-radius: 999px;
  background: var(--p-surface-800);
  color: var(--p-text-muted-color);
  font-size: 0.75rem;
}

.outcome {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0 0.5rem;
  border-radius: 999px;
  font-size: 0.8125rem;
  white-space: nowrap;
}

.outcome.ok {
  background: color-mix(in srgb, var(--p-green-400) 18%, transparent);
  color: var(--p-green-300);
}

.outcome.bad {
  background: color-mix(in srgb, var(--p-yellow-400) 18%, transparent);
  color: var(--p-yellow-300);
}

.outcome.wait,
.outcome.skip {
  background: var(--p-surface-800);
  color: var(--p-text-muted-color);
}
</style>
