<template>
  <!-- 模擬事件 (docs/decisions.md 管理 UI 版面): one test event per overlay, to check each one works in OBS. -->
  <dialog ref="dialog" class="simulate sheet" aria-labelledby="simulate-title" @close="onClose">
    <div class="head">
      <h2 id="simulate-title">{{ t('simulate.dialog.title') }}</h2>
      <Button ref="closeButton" :label="t('app.close')" severity="secondary" outlined size="small" @click="dialog?.close()" />
    </div>
    <ul class="plain rows">
      <li v-for="row in rows" :key="row.id" :data-row="row.id">
        <span class="what">{{ t(`simulate.dialog.rows.${row.id}`) }}</span>
        <span class="actions">
          <template v-if="row.id === 'sound'">
            <Select
              v-model="soundId"
              :options="sounds"
              option-label="label"
              option-value="id"
              :placeholder="t('simulate.dialog.pickSound')"
              :aria-label="t('simulate.dialog.pickSound')"
              :disabled="!row.on || !sounds.length"
              size="small"
              append-to="self"
            />
            <Button
              :label="t('simulate.dialog.play')"
              size="small"
              outlined
              :disabled="!row.on || !sounds.some((s) => s.id === soundId) || busy"
              @click="send(row.id, t('simulate.dialog.play'), () => testSound(soundId!))"
            />
          </template>
          <Button
            v-for="b in row.buttons"
            v-else
            :key="b.label"
            :label="b.label"
            size="small"
            outlined
            :disabled="!row.on || b.disabled || busy"
            @click="send(row.id, b.label, b.run)"
          />
        </span>
        <span class="state">
          <span v-if="modules && !row.on" class="off">{{ t('simulate.dialog.featureOff') }}</span>
          <!-- Feature states not read yet (or the read failed): nothing can be sent, and it says so. -->
          <span v-else-if="!modules || connected[row.overlay] === undefined" class="unknown">
            <i class="pi pi-question-circle" aria-hidden="true"></i>{{ t('simulate.dialog.unknown') }}
          </span>
          <span v-else-if="connected[row.overlay]" class="ok">
            <i class="pi pi-check-circle" aria-hidden="true"></i>{{ t('simulate.dialog.connected') }}
          </span>
          <span v-else class="missing">
            <i class="pi pi-exclamation-triangle" aria-hidden="true"></i>{{ t('simulate.dialog.notInObs') }}
          </span>
        </span>
      </li>
    </ul>
    <p class="result" :class="{ error: result?.error }" role="status">{{ result?.text }}</p>
  </dialog>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Select from 'primevue/select'
import { computed, nextTick, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ALERT_KINDS } from '../overlay/alerts/alerts'
import { ApiError, type Schemas, api, client } from './api'
import { FEATURES, isOn, useModules } from './features'
import { ALERT_AMOUNTS, simulateAlert, simulateChat, simulateCheckin, testSound } from './simulate'

// How often the dialog asks which overlays are open in OBS, while it is open.
const OVERLAYS_MS = 3000

const emit = defineEmits<{ closed: [] }>()
const { t } = useI18n()
const { modules } = useModules()
const dialog = ref<HTMLDialogElement>()
const closeButton = ref<{ $el: HTMLElement }>()
const busy = ref(false)
const result = ref<{ text: string; error: boolean }>()
const sounds = shallowRef<{ id: string; label: string }[]>([])
const soundId = ref<string>()
// `undefined` until the card's slot count is read: 集滿 waits for it.
const cardSlots = ref<number>()
// Overlay connections from `GET /api/admin/overlays`; a kind is `undefined` while that could not be read.
const connected = ref<Partial<Record<string, boolean>>>({})
let timer: number | undefined

type Row = {
  id: string
  module: string
  overlay: string
  buttons: { label: string; run: () => Promise<unknown>; disabled?: boolean }[]
}
const on = (module: string) => {
  const feature = FEATURES.find((f) => f.module === module)
  return !!modules.value && !!feature && isOn(feature, modules.value)
}
const rows = computed(() =>
  (
    [
      {
        id: 'chat',
        module: 'chat',
        overlay: 'chat',
        buttons: [{ label: t('simulate.dialog.sendMessage'), run: () => simulateChat({ text: t('simulate.dialog.chatText') }) }],
      },
      {
        id: 'alerts',
        module: 'alerts',
        overlay: 'alerts',
        buttons: ALERT_KINDS.map((kind) => ({
          label: t(`alerts.kinds.${kind}`),
          run: () => simulateAlert({ kind, amount: ALERT_AMOUNTS[kind] ?? null }),
        })),
      },
      { id: 'sound', module: 'sound', overlay: 'sound', buttons: [] },
      {
        id: 'checkin',
        module: 'checkin',
        overlay: 'checkin',
        buttons: [
          { label: t('simulate.dialog.checkin'), run: () => simulateCheckin({ stamps: 1 }) },
          // With one slot a first stamp already fills the card.
          {
            label: t('simulate.dialog.fullCard'),
            run: () => simulateCheckin({ stamps: cardSlots.value! }),
            disabled: cardSlots.value === undefined,
          },
        ],
      },
    ] satisfies Row[]
  ).map((row: Row) => ({ ...row, on: on(row.module) })),
)

async function send(row: string, button: string, run: () => Promise<unknown>) {
  busy.value = true
  try {
    await run()
    result.value = { text: t('simulate.dialog.sent', { row: t(`simulate.dialog.rows.${row}`), button }), error: false }
  } catch (e) {
    result.value = { text: e instanceof ApiError ? errorMessage(e.code) : String(e), error: true }
  } finally {
    busy.value = false
  }
}

async function loadConnections() {
  try {
    const counts = await api(client.GET('/api/admin/overlays'))
    connected.value = Object.fromEntries(Object.entries(counts).map(([kind, n]) => [kind, (n as number) > 0]))
  } catch {
    connected.value = {}
  }
}

async function loadChoices() {
  try {
    const library: Schemas['SoundLibrary'] = await api(client.GET('/api/admin/sounds'))
    sounds.value = library.sounds.map((s) => ({ id: s.id, label: s.label }))
    // A sound deleted since the last time is no longer offered.
    if (!sounds.value.some((s) => s.id === soundId.value)) soundId.value = sounds.value[0]?.id
  } catch {
    sounds.value = []
    soundId.value = undefined
  }
  try {
    cardSlots.value = (await api(client.GET('/api/admin/checkin/settings'))).card_slots
  } catch {
    // Unknown, so 集滿 stays off rather than guess.
    cardSlots.value = undefined
  }
}

/** Opens the dialog with focus on 關閉, and keeps the connections current while it is open. */
async function open() {
  result.value = undefined
  dialog.value?.showModal()
  await nextTick()
  closeButton.value?.$el.focus()
  void loadChoices()
  void loadConnections()
  timer = window.setInterval(() => void loadConnections(), OVERLAYS_MS)
}

function onClose() {
  window.clearInterval(timer)
  emit('closed')
}
onBeforeUnmount(() => window.clearInterval(timer))

defineExpose({ open })
</script>

<style scoped>
.simulate {
  width: min(48rem, calc(100vw - 2rem));
  padding: 0;
  border: 1px solid var(--p-surface-500);
  border-radius: 8px;
  background: var(--p-surface-900);
  color: inherit;
}

.simulate::backdrop {
  background: rgb(0 0 0 / 60%);
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.9rem 1.1rem;
  border-bottom: 1px solid var(--p-surface-800);
}

.head h2 {
  margin: 0;
}

.plain {
  list-style: none;
  margin: 0;
  padding: 0;
}

.rows {
  padding: 0.25rem 1.1rem;
}

.rows li {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
  padding: 0.6rem 0;
}

.rows li + li {
  border-top: 1px solid var(--p-surface-800);
}

.what {
  min-width: 6rem;
  font-weight: 600;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
}

.state {
  margin-left: auto;
  font-size: 0.875rem;
}

.state > span {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
}

.ok {
  color: var(--p-green-400);
}

.missing {
  color: var(--p-yellow-400);
}

.unknown,
.off {
  color: var(--p-text-muted-color);
}

.result {
  min-height: 1.5rem;
  margin: 0;
  padding: 0 1.1rem 1rem;
}

.result.error {
  color: var(--p-red-400);
}
</style>
