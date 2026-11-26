<template>
  <!-- 新增／編輯獎勵 (prototype dlg-reward): a toolkit reward's settings on Twitch, and deleting it. Twitch's limits are
       checked here first (and again by the Host), so a refusal is explained before anything is sent. -->
  <dialog ref="dialog" class="sheet reward-dialog" aria-labelledby="rwd-title" @close="onClose">
    <template v-if="!deleting">
      <h2 id="rwd-title">{{ t(editing ? 'rewards.dialog.editTitle' : 'rewards.dialog.addTitle') }}</h2>
      <form id="rwd-form" @submit.prevent="save">
        <FormRow :label="t('rewards.dialog.name')" input-id="rwd-name">
          <InputText id="rwd-name" v-model="form.title" :maxlength="TITLE_MAX" required :invalid="!!titleProblem" aria-describedby="rwd-name-problem" />
          <template v-if="titleProblem" #hint><span id="rwd-name-problem">{{ titleProblem }}</span></template>
        </FormRow>
        <FormRow :label="t('rewards.dialog.prompt')" input-id="rwd-prompt">
          <InputText id="rwd-prompt" v-model="form.prompt" :maxlength="PROMPT_MAX" />
        </FormRow>
        <FormRow :label="t('rewards.cost')" input-id="rwd-cost">
          <InputNumber v-model="form.cost" input-id="rwd-cost" :min="1" :max="COUNT_MAX" :use-grouping="false" />
        </FormRow>
        <FormRow :label="t('rewards.dialog.color')" input-id="rwd-color">
          <input id="rwd-color" v-model="form.color" type="color" class="color" />
        </FormRow>
        <FormRow :label="t('rewards.dialog.options')" label-id="rwd-options" top>
          <div class="checks" role="group" aria-labelledby="rwd-options">
            <label><Checkbox v-model="form.input" binary />{{ t('rewards.dialog.input') }}</label>
            <label><Checkbox v-model="form.skipQueue" binary />{{ t('rewards.dialog.skipQueue') }}</label>
          </div>
        </FormRow>
        <FormRow :label="t('rewards.cooldown')" input-id="rwd-cooldown">
          <Select v-model="form.cooldown" input-id="rwd-cooldown" :options="cooldowns" option-label="label" option-value="value" />
        </FormRow>
        <FormRow :label="t('rewards.dialog.maxPerStream')" input-id="rwd-max">
          <InputNumber v-model="form.maxPerStream" input-id="rwd-max" :min="1" :max="COUNT_MAX" :use-grouping="false" :placeholder="t('rewards.dialog.noLimit')" />
        </FormRow>
        <FormRow :label="t('rewards.dialog.maxPerUser')" input-id="rwd-maxu">
          <InputNumber v-model="form.maxPerUser" input-id="rwd-maxu" :min="1" :max="COUNT_MAX" :use-grouping="false" :placeholder="t('rewards.dialog.noLimit')" />
        </FormRow>
      </form>
      <p v-if="failed" class="failed" role="alert">{{ failed }}</p>
      <div class="buttons sheet-actions">
        <Button
          v-if="editing"
          :label="t('rewards.dialog.delete')"
          icon="pi pi-trash"
          severity="danger"
          text
          class="delete"
          :disabled="busy"
          @click="deleting = true"
        />
        <Button
          :label="t('rewards.dialog.save')"
          type="submit"
          form="rwd-form"
          :disabled="busy || !form.title.trim() || !form.cost || !!titleProblem"
        />
        <Button :label="t('features.cancel')" text @click="dialog?.close()" />
      </div>
    </template>
    <template v-else>
      <h2 id="rwd-title">{{ t('rewards.dialog.deleteTitle', { name: editing?.title }) }}</h2>
      <p>{{ t('rewards.dialog.deleteText') }}</p>
      <ul v-if="uses.length">
        <li v-for="u in uses" :key="u">{{ u }}</li>
      </ul>
      <p v-if="failed" class="failed" role="alert">{{ failed }}</p>
      <div class="buttons sheet-actions">
        <Button :label="t('rewards.dialog.delete')" severity="danger" :disabled="busy" autofocus @click="remove" />
        <Button :label="t('features.cancel')" text @click="deleting = false" />
      </div>
    </template>
  </dialog>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FormRow from './FormRow.vue'
import { successToast } from './home'

type Reward = Schemas['ChannelReward']
// Twitch's limits, checked again by the Host.
const TITLE_MAX = 45
const PROMPT_MAX = 200
const COUNT_MAX = 2147483647
const COOLDOWNS = [0, 60, 300, 600]

const emit = defineEmits<{ saved: [] }>()
const { t } = useI18n()
const toast = useToast()
const dialog = ref<HTMLDialogElement>()
const editing = ref<Reward>()
const others = ref<string[]>([])
const uses = ref<string[]>([])
const deleting = ref(false)
const busy = ref(false)
const failed = ref('')
const form = reactive({
  title: '',
  prompt: '',
  cost: null as number | null,
  color: '#9146FF',
  input: false,
  skipQueue: false,
  cooldown: 0,
  maxPerStream: null as number | null,
  maxPerUser: null as number | null,
})

const minutes = (s: number) => (s % 60 ? t('rewards.seconds', { n: s }) : t('rewards.minutes', { n: s / 60 }))
// A reward's own cooldown stays a choice even when it is not one of the usual ones.
const cooldowns = computed(() =>
  [...new Set([...COOLDOWNS, form.cooldown])]
    .sort((a, b) => a - b)
    .map((value) => ({ value, label: value ? minutes(value) : t('rewards.dialog.noCooldown') })),
)
// Twitch refuses a second reward with the same title, ignoring case.
const titleProblem = computed(() =>
  others.value.includes(form.title.trim().toLowerCase()) ? t('rewards.dialog.titleTaken') : '',
)

/** `reward`: edit it; none: add one. `titles`: every reward's title on the channel; `used`: what uses it. */
function open(reward: Reward | undefined, titles: string[], used: string[] = []) {
  editing.value = reward
  others.value = titles.filter((x) => x !== reward?.title).map((x) => x.trim().toLowerCase())
  uses.value = used
  Object.assign(form, {
    title: reward?.title ?? '',
    prompt: reward?.prompt ?? '',
    cost: reward?.cost ?? null,
    color: reward?.background_color || '#9146FF',
    input: reward?.is_user_input_required ?? false,
    skipQueue: reward?.skip_queue ?? false,
    cooldown: reward?.cooldown_seconds ?? 0,
    maxPerStream: reward?.max_per_stream ?? null,
    maxPerUser: reward?.max_per_user_per_stream ?? null,
  })
  deleting.value = false
  failed.value = ''
  dialog.value!.returnValue = ''
  dialog.value!.showModal()
}
defineExpose({ open })

function onClose() {
  deleting.value = false
}

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))

async function run(action: () => Promise<unknown>, done: string) {
  busy.value = true
  failed.value = ''
  try {
    await action()
    toast.add(successToast(done))
    dialog.value?.close()
    emit('saved')
  } catch (e) {
    failed.value = message(e)
  } finally {
    busy.value = false
  }
}

function save() {
  if (titleProblem.value || !form.cost) return
  const body = {
    title: form.title.trim(),
    prompt: form.prompt.trim(),
    cost: form.cost,
    background_color: form.color,
    is_user_input_required: form.input,
    skip_queue: form.skipQueue,
    cooldown_seconds: form.cooldown,
    max_per_stream: form.maxPerStream ?? 0,
    max_per_user_per_stream: form.maxPerUser ?? 0,
  }
  const reward = editing.value
  return run(
    () =>
      api(
        reward
          ? client.PATCH('/api/admin/rewards/{id}', { params: { path: { id: reward.id } }, body })
          : client.POST('/api/admin/rewards', { body }),
      ),
    t(reward ? 'rewards.dialog.updated' : 'rewards.dialog.added'),
  )
}

function remove() {
  const reward = editing.value!
  return run(
    () => api(client.DELETE('/api/admin/rewards/{id}', { params: { path: { id: reward.id } } })),
    t('rewards.dialog.deleted', { name: reward.title }),
  )
}
</script>

<style scoped>
.reward-dialog {
  width: min(40rem, calc(100vw - 2rem));
}

.checks {
  display: grid;
  gap: 0.5rem;
}

.checks label {
  display: inline-flex;
  gap: 0.5rem;
  align-items: center;
}

.color {
  width: 3rem;
  height: 2.25rem;
  padding: 0.125rem;
  border: 1px solid var(--p-surface-600);
  border-radius: 6px;
  background: none;
}

.delete {
  margin-inline-end: auto;
}

.failed {
  color: var(--p-red-400);
}
</style>
