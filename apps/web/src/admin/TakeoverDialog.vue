<template>
  <!-- 接管 (prototype dlg-takeover): a toolkit copy of a reward made in the Twitch dashboard, under a new title (Twitch
       refuses two with the same one), with what used the original moved to it. -->
  <dialog ref="dialog" class="sheet takeover-dialog" aria-labelledby="tk-title">
    <h2 id="tk-title">{{ t('rewards.takeover.title', { name: original?.title }) }}</h2>
    <p>{{ t('rewards.takeover.text') }}</p>
    <form id="tk-form" @submit.prevent="takeOver">
      <label for="tk-name" class="label">{{ t('rewards.takeover.name') }}</label>
      <InputText id="tk-name" v-model="title" :maxlength="TITLE_MAX" :invalid="!!problem" aria-describedby="tk-help" />
      <p id="tk-help" class="help">{{ problem || t('rewards.takeover.help') }}</p>
    </form>
    <p v-if="failed" class="failed" role="alert">{{ failed }}</p>
    <div class="buttons sheet-actions">
      <Button :label="t('rewards.takeover.ok')" type="submit" form="tk-form" :disabled="busy || !title.trim() || !!problem" />
      <Button :label="t('features.cancel')" text autofocus @click="dialog?.close()" />
    </div>
  </dialog>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { successToast } from './home'

const TITLE_MAX = 45

const emit = defineEmits<{ done: [] }>()
const { t } = useI18n()
const toast = useToast()
const dialog = ref<HTMLDialogElement>()
const original = ref<Schemas['ChannelReward']>()
const titles = ref<string[]>([])
const title = ref('')
const busy = ref(false)
const failed = ref('')

// Twitch refuses a second reward with the same title (the original's included), ignoring case.
const problem = computed(() =>
  titles.value.includes(title.value.trim().toLowerCase()) ? t('rewards.takeover.sameName') : '',
)

/** `all`: every reward's title on the channel, the original's included. */
function open(reward: Schemas['ChannelReward'], all: string[]) {
  original.value = reward
  titles.value = all.map((x) => x.trim().toLowerCase())
  title.value = reward.title
  failed.value = ''
  dialog.value!.showModal()
}
defineExpose({ open })

async function takeOver() {
  const reward = original.value
  if (!reward || problem.value) return
  busy.value = true
  failed.value = ''
  try {
    const done = await api(
      client.POST('/api/admin/rewards/{id}/takeover', { params: { path: { id: reward.id } }, body: { title: title.value.trim() } }),
    )
    const name = reward.title
    if (done.not_moved.length) {
      const uses = done.not_moved.map((u) => t(`rewards.takeover.uses.${u}`)).join('、')
      toast.add({ severity: 'warn', summary: t('rewards.takeover.partly', { name, uses }) })
    } else {
      toast.add(successToast(t('rewards.takeover.done', { name })))
    }
    if (done.pause_failed) toast.add({ severity: 'warn', summary: t('rewards.takeover.notPaused', { name: title.value.trim() }) })
    dialog.value?.close()
    emit('done')
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.takeover-dialog {
  width: min(32rem, calc(100vw - 2rem));
}

form {
  display: grid;
  gap: 0.375rem;
}

.help {
  margin: 0;
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.failed {
  color: var(--p-red-400);
}
</style>
