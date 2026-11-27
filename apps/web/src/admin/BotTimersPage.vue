<template>
  <!-- 聊天機器人 › 定時訊息: messages posted on a timer while live, skipped when chat was too quiet. -->
  <FullPage :status="t('bot.summary')">
    <div class="bot">
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
      <section class="tbl-wrap" aria-labelledby="bot-timers">
        <h3 id="bot-timers" class="tbl-head">{{ t('bot.timers.heading') }}</h3>
        <p v-if="!timers.length" class="empty-cell">{{ t('bot.timers.empty') }}</p>
        <table v-else class="table">
          <thead>
            <tr>
              <th scope="col">{{ t('bot.timers.name') }}</th>
              <th scope="col">{{ t('bot.timers.message') }}</th>
              <th scope="col">{{ t('bot.timers.every') }}</th>
              <th scope="col">{{ t('bot.timers.minLines') }}</th>
              <th scope="col">{{ t('bot.timers.next') }}</th>
              <th scope="col" class="num">{{ t('bot.commands.enabled') }}</th>
              <th scope="col"><span class="sr-only">{{ t('bot.commands.actions') }}</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="tm in timers" :key="tm.id" :data-timer="tm.id">
              <th scope="row">{{ tm.name }}</th>
              <td class="message">{{ tm.message }}</td>
              <td>{{ t('bot.minutes', { n: tm.every_minutes }) }}</td>
              <td>{{ t('bot.timers.lines', { n: tm.min_chat_lines }) }}</td>
              <td data-next>{{ nextText(tm) }}</td>
              <td class="num">
                <ToggleSwitch
                  :model-value="tm.enabled"
                  :disabled="busy"
                  :aria-label="t('bot.timers.enableLabel', { name: tm.name })"
                  @update:model-value="save(tm.id, { ...body(tm), enabled: $event })"
                />
              </td>
              <td class="num">
                <Button icon="pi pi-pencil" text :aria-label="t('bot.timers.editLabel', { name: tm.name })" @click="edit(tm)" />
              </td>
            </tr>
          </tbody>
        </table>
        <div class="tbl-foot">
          <span class="note">{{ t('bot.timers.note') }}</span>
          <Button :label="t('bot.timers.add')" icon="pi pi-plus" :disabled="timers.length >= MAX_TIMERS" @click="edit()" />
        </div>
      </section>
    </div>

    <dialog ref="dialog" class="sheet timer-dialog" aria-labelledby="timer-title">
      <template v-if="!deleting">
        <h2 id="timer-title">{{ t(editing ? 'bot.timers.editTitle' : 'bot.timers.addTitle') }}</h2>
        <form id="timer-form" @submit.prevent="submit">
          <FormRow :label="t('bot.timers.name')" input-id="timer-name">
            <InputText id="timer-name" v-model="form.name" :maxlength="NAME_MAX" />
          </FormRow>
          <FormRow :label="t('bot.timers.message')" input-id="timer-message" top>
            <Textarea id="timer-message" v-model="form.message" :maxlength="MESSAGE_MAX" rows="3" />
          </FormRow>
          <FormRow :label="t('bot.timers.every')" input-id="timer-every">
            <InputNumber v-model="form.every_minutes" input-id="timer-every" :min="5" :max="120" :suffix="` ${t('bot.timers.minutesUnit')}`" />
          </FormRow>
          <FormRow :label="t('bot.timers.minLines')" input-id="timer-lines">
            <InputNumber v-model="form.min_chat_lines" input-id="timer-lines" :min="0" :max="100" :suffix="` ${t('bot.timers.linesUnit')}`" />
          </FormRow>
        </form>
        <p v-if="failed" class="failed" role="alert">{{ failed }}</p>
        <div class="buttons sheet-actions">
          <Button v-if="editing" :label="t('bot.commands.delete')" icon="pi pi-trash" severity="danger" text class="delete" @click="deleting = true" />
          <Button :label="t('bot.commands.save')" type="submit" form="timer-form" :disabled="busy || !form.name.trim() || !form.message.trim()" />
          <Button :label="t('features.cancel')" text @click="dialog?.close()" />
        </div>
      </template>
      <template v-else>
        <h2 id="timer-title">{{ t('bot.timers.deleteTitle', { name: editing?.name }) }}</h2>
        <p v-if="failed" class="failed" role="alert">{{ failed }}</p>
        <div class="buttons sheet-actions">
          <Button :label="t('bot.commands.delete')" severity="danger" autofocus @click="remove" />
          <Button :label="t('features.cancel')" text @click="deleting = false" />
        </div>
      </template>
    </dialog>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import Textarea from 'primevue/textarea'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { onBeforeUnmount, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import { errorToast } from './home'

type Timer = Schemas['BotTimerView']
type Body = Schemas['BotTimerBody']
// The Host's limits, checked again there.
const NAME_MAX = 50
const MESSAGE_MAX = 500
const MAX_TIMERS = 20
// The next times move along while live.
const REFRESH_MS = 30_000

const { t } = useI18n()
const toast = useToast()
const timers = ref<Timer[]>([])
const loadFailed = ref('')
const busy = ref(false)
const failed = ref('')
const dialog = ref<HTMLDialogElement>()
const editing = ref<Timer>()
const deleting = ref(false)
const blank = (): Body => ({ name: '', message: '', every_minutes: 15, min_chat_lines: 5, enabled: true })
const form = reactive<Body>(blank())

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))
const body = (tm: Timer): Body => ({
  name: tm.name,
  message: tm.message,
  every_minutes: tm.every_minutes,
  min_chat_lines: tm.min_chat_lines,
  enabled: tm.enabled,
})
function nextText(tm: Timer) {
  if (!tm.enabled) return '—'
  if (!tm.next_at) return t('bot.timers.offline')
  const minutes = Math.max(0, Math.round((Date.parse(tm.next_at) - Date.now()) / 60_000))
  return minutes ? t('bot.timers.inMinutes', { n: minutes }) : t('bot.timers.soon')
}

async function load() {
  timers.value = await api(client.GET('/api/admin/bot/timers'))
}

function edit(tm?: Timer) {
  editing.value = tm
  Object.assign(form, tm ? body(tm) : blank())
  deleting.value = false
  failed.value = ''
  dialog.value!.showModal()
}

async function save(id: string | undefined, next: Body) {
  busy.value = true
  failed.value = ''
  try {
    await api(
      id
        ? client.PUT('/api/admin/bot/timers/{id}', { params: { path: { id } }, body: next })
        : client.POST('/api/admin/bot/timers', { body: next }),
    )
    await load()
    return true
  } catch (e) {
    if (dialog.value?.open) failed.value = message(e)
    else toast.add(errorToast(message(e)))
    return false
  } finally {
    busy.value = false
  }
}

async function submit() {
  if (await save(editing.value?.id, { ...form })) dialog.value?.close()
}

async function remove() {
  const id = editing.value!.id
  try {
    await api(client.DELETE('/api/admin/bot/timers/{id}', { params: { path: { id } } }))
    dialog.value?.close()
    await load()
  } catch (e) {
    failed.value = message(e)
  }
}

load().catch((e: unknown) => (loadFailed.value = message(e)))
const refresh = window.setInterval(() => void load().catch(() => {}), REFRESH_MS)
onBeforeUnmount(() => window.clearInterval(refresh))
</script>

<style scoped>
.bot {
  max-width: 72rem;
}

.message {
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}

.num {
  text-align: end;
}

.note {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.timer-dialog {
  width: min(40rem, calc(100vw - 2rem));
}

.timer-dialog .p-textarea {
  width: 100%;
}

.delete {
  margin-inline-end: auto;
}

.failed {
  color: var(--p-red-400);
}
</style>
