<template>
  <!-- 聊天機器人 › 指令: commands the bot answers as the streamer (docs/modules/chat-bot.md). -->
  <FullPage :status="t('bot.summary')">
    <div class="bot">
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
      <p v-if="dropped" class="note" data-dropped>{{ t('bot.dropped', { n: dropped }) }}</p>
      <section class="tbl-wrap" aria-labelledby="bot-commands">
        <h3 id="bot-commands" class="tbl-head">{{ t('bot.commands.heading') }}</h3>
        <p v-if="!commands.length" class="empty-cell">{{ t('bot.commands.empty') }}</p>
        <table v-else class="table">
          <thead>
            <tr>
              <th scope="col">{{ t('bot.commands.name') }}</th>
              <th scope="col">{{ t('bot.commands.reply') }}</th>
              <th scope="col">{{ t('bot.commands.allowed') }}</th>
              <th scope="col">{{ t('bot.commands.cooldown') }}</th>
              <th scope="col" class="num">{{ t('bot.commands.enabled') }}</th>
              <th scope="col"><span class="sr-only">{{ t('bot.commands.actions') }}</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="c in commands" :key="c.id" :data-command="c.id">
              <th scope="row"><code>!{{ c.name }}</code></th>
              <td class="reply">{{ c.reply }}</td>
              <td>{{ t(`commands.allowed.${c.allowed}`) }}</td>
              <td>{{ cooldown(c.cooldown_seconds) }}</td>
              <td class="num">
                <ToggleSwitch
                  :model-value="c.enabled"
                  :disabled="busy"
                  :aria-label="t('bot.commands.enableLabel', { name: c.name })"
                  @update:model-value="save(c.id, { ...body(c), enabled: $event })"
                />
              </td>
              <td class="num">
                <Button icon="pi pi-pencil" text :aria-label="t('bot.commands.editLabel', { name: c.name })" @click="edit(c)" />
              </td>
            </tr>
          </tbody>
        </table>
        <div class="tbl-foot">
          <span class="note">{{ t('bot.commands.note') }}</span>
          <Button :label="t('bot.commands.add')" icon="pi pi-plus" :disabled="commands.length >= MAX_COMMANDS" @click="edit()" />
        </div>
      </section>
    </div>

    <dialog ref="dialog" class="sheet command-dialog" aria-labelledby="cmd-title">
      <template v-if="!deleting">
        <h2 id="cmd-title">{{ t(editing ? 'bot.commands.editTitle' : 'bot.commands.addTitle') }}</h2>
        <form id="cmd-form" @submit.prevent="submit">
          <FormRow :label="t('bot.commands.name')" input-id="cmd-name">
            <InputText id="cmd-name" v-model="form.name" :maxlength="NAME_MAX + 1" placeholder="!discord" />
          </FormRow>
          <FormRow :label="t('bot.commands.reply')" input-id="cmd-reply" top>
            <Textarea id="cmd-reply" v-model="form.reply" :maxlength="REPLY_MAX" rows="3" />
            <template #hint>{{ t('bot.commands.note') }}</template>
          </FormRow>
          <FormRow :label="t('bot.commands.allowed')" input-id="cmd-allowed">
            <Select v-model="form.allowed" input-id="cmd-allowed" :options="allowedOptions" option-label="label" option-value="value" />
          </FormRow>
          <FormRow :label="t('bot.commands.cooldown')" input-id="cmd-cooldown">
            <Select v-model="form.cooldown_seconds" input-id="cmd-cooldown" :options="cooldowns" option-label="label" option-value="value" />
          </FormRow>
        </form>
        <p v-if="failed" class="failed" role="alert">{{ failed }}</p>
        <div class="buttons sheet-actions">
          <Button v-if="editing" :label="t('bot.commands.delete')" icon="pi pi-trash" severity="danger" text class="delete" @click="deleting = true" />
          <Button :label="t('bot.commands.save')" type="submit" form="cmd-form" :disabled="busy || !form.name.trim() || !form.reply.trim()" />
          <Button :label="t('features.cancel')" text @click="dialog?.close()" />
        </div>
      </template>
      <template v-else>
        <h2 id="cmd-title">{{ t('bot.commands.deleteTitle', { name: editing?.name }) }}</h2>
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
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import Textarea from 'primevue/textarea'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { computed, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import { errorToast } from './home'

type Command = Schemas['BotCommandView']
type Body = Schemas['BotCommandBody']
// The Host's limits, checked again there.
const NAME_MAX = 25
const REPLY_MAX = 500
const MAX_COMMANDS = 100
const COOLDOWNS = [0, 10, 30, 60, 300]
const ALLOWED: Schemas['Allowed'][] = ['everyone', 'subscriber', 'vip', 'moderator']

const { t } = useI18n()
const toast = useToast()
const commands = ref<Command[]>([])
const dropped = ref(0)
const loadFailed = ref('')
const busy = ref(false)
const failed = ref('')
const dialog = ref<HTMLDialogElement>()
const editing = ref<Command>()
const deleting = ref(false)
const form = reactive<Body>({ name: '', reply: '', allowed: 'everyone', cooldown_seconds: 30, enabled: true })

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))
const cooldown = (s: number) => (!s ? t('bot.commands.noCooldown') : s % 60 ? t('bot.seconds', { n: s }) : t('bot.minutes', { n: s / 60 }))
// A command's own cooldown stays a choice even when it is not one of the usual ones.
const cooldowns = computed(() =>
  [...new Set([...COOLDOWNS, form.cooldown_seconds])].sort((a, b) => a - b).map((value) => ({ value, label: cooldown(value) })),
)
const allowedOptions = computed(() => ALLOWED.map((value) => ({ value, label: t(`commands.allowed.${value}`) })))
const body = (c: Command): Body => ({ name: c.name, reply: c.reply, allowed: c.allowed, cooldown_seconds: c.cooldown_seconds, enabled: c.enabled })

async function load() {
  commands.value = await api(client.GET('/api/admin/bot/commands'))
}

function edit(c?: Command) {
  editing.value = c
  Object.assign(form, c ? body(c) : { name: '', reply: '', allowed: 'everyone', cooldown_seconds: 30, enabled: true })
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
        ? client.PUT('/api/admin/bot/commands/{id}', { params: { path: { id } }, body: next })
        : client.POST('/api/admin/bot/commands', { body: next }),
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
    await api(client.DELETE('/api/admin/bot/commands/{id}', { params: { path: { id } } }))
    dialog.value?.close()
    await load()
  } catch (e) {
    failed.value = message(e)
  }
}

load().catch((e: unknown) => (loadFailed.value = message(e)))
void api(client.GET('/api/admin/bot/status')).then((s) => (dropped.value = s.dropped), () => {})
</script>

<style scoped>
.bot {
  max-width: 72rem;
}

.reply {
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

.command-dialog {
  width: min(40rem, calc(100vw - 2rem));
}

.command-dialog .p-textarea {
  width: 100%;
}

.delete {
  margin-inline-end: auto;
}

.failed {
  color: var(--p-red-400);
}
</style>
