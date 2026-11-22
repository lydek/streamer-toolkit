<template>
  <div class="stack">
    <div class="field">
      <span id="backup-choice">{{ t('backup.choice') }}</span>
      <SelectButton
        v-model="choice"
        :options="CHOICES"
        :option-label="choiceLabel"
        :allow-empty="false"
        aria-labelledby="backup-choice"
      />
    </div>
    <Button :label="t('backup.export')" icon="pi pi-download" :disabled="busy" @click="exportBackup" />
    <label for="restore-file">{{ t('backup.restoreFile') }}</label>
    <input id="restore-file" ref="input" type="file" accept=".stbackup" :disabled="busy" @change="choose" />
    <div v-if="progress" class="progress">
      <p role="status">{{ progressText }}</p>
      <ProgressBar :value="progressPercent" :mode="progress.phase === 'check' ? 'indeterminate' : 'determinate'" />
      <Button :label="t('backup.cancel')" severity="secondary" text @click="controller?.abort()" />
    </div>
    <p v-if="error" role="alert">{{ error }}</p>
    <template v-if="preview">
      <ul class="preview">
        <li>
          {{ t('backup.createdAt') }}<time :datetime="preview.created_at">{{ formatTime(preview.created_at) }}</time>
        </li>
        <li>{{ t('backup.appVersion', { version: preview.app_version }) }}</li>
        <li>{{ t('backup.contains', { kinds: preview.contents.map((c) => t(`backup.choices.${c}`)).join('、') }) }}</li>
        <li v-if="preview.contents.includes('data')">
          {{ t('backup.records', { redemptions: preview.redemptions, checkins: preview.checkins }) }}
        </li>
        <li v-if="preview.contents.includes('media')">
          {{ t('backup.files', { templates: preview.templates, sounds: preview.sounds, music: preview.music, media: preview.media }) }}
        </li>
      </ul>
      <p>{{ t(preview.contents.length === 1 ? `backup.restoreOnly.${preview.contents[0]}` : 'backup.restoreNote') }}</p>
      <Button :label="t('backup.restore')" icon="pi pi-replay" :disabled="busy" @click="restore" />
    </template>
    <p v-if="restarting" role="status">{{ t('backup.restarting') }}</p>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import ProgressBar from 'primevue/progressbar'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import { computed, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas } from './api'
import {
  type BackupChoice,
  type RestoreProgress,
  backupError,
  backupSizes,
  confirmRestore,
  formatSize,
  previewRestore,
  saveBackup,
  sizeUnit,
} from './backup'
import { errorToast, successToast } from './home'

const { t, locale } = useI18n()
const toast = useToast()
const busy = ref(false)
const input = ref<HTMLInputElement>()
const preview = shallowRef<Schemas['RestorePreview']>()
const error = ref('')
const restarting = ref(false)
const CHOICES: BackupChoice[] = ['all', 'data', 'media']
const choice = ref<BackupChoice>('all')
const sizes = shallowRef<Schemas['BackupSizes']>()
const progress = shallowRef<RestoreProgress>()
let controller: AbortController | undefined

const message = backupError
const progressPercent = computed(() => (progress.value ? Math.floor((progress.value.sent / progress.value.total) * 100) : 0))
const progressText = computed(() => {
  const p = progress.value
  if (!p || p.phase === 'check') return t('backup.checking')
  // Both in the total's unit: 上傳中 1.2 / 3.4 GB.
  const unit = sizeUnit(p.total)
  return t('backup.uploading', { sent: formatSize(p.sent, unit).split(' ')[0], total: formatSize(p.total, unit) })
})
onBeforeUnmount(() => controller?.abort())
// Only a hint for choosing: without it the page works the same.
backupSizes().then((s) => (sizes.value = s), () => {})
/** Each choice with its size, when the sizes are known: 「資料 12 MB」. */
function choiceLabel(c: BackupChoice) {
  const s = sizes.value
  const bytes = !s ? undefined : c === 'all' ? s.data + s.media : c === 'data' ? s.data : s.media
  return bytes === undefined ? t(`backup.choices.${c}`) : t('backup.choiceSize', { choice: t(`backup.choices.${c}`), size: formatSize(bytes) })
}
const formatTime = (iso: string) =>
  new Date(iso).toLocaleString(locale.value, { dateStyle: 'medium', timeStyle: 'short' })

async function exportBackup() {
  busy.value = true
  try {
    const saved = await saveBackup(choice.value)
    if (saved) toast.add(successToast(t('backup.exported', { file: saved })))
  } catch (e) {
    toast.add(errorToast(message(e)))
  } finally {
    busy.value = false
  }
}

// The Host checks and stages the file; nothing in use changes until the restore is confirmed.
async function choose() {
  preview.value = undefined
  error.value = ''
  const file = input.value?.files?.[0]
  if (!file) return
  busy.value = true
  controller = new AbortController()
  const { signal } = controller
  try {
    preview.value = await previewRestore(file, (p) => (progress.value = p), signal)
  } catch (e) {
    refuse(signal.aborted ? '' : message(e))
  } finally {
    progress.value = undefined
    controller = undefined
    busy.value = false
  }
}

/** Shows why the file was refused and clears the choice, so choosing the same file once fixed fires again. */
function refuse(text: string) {
  error.value = text
  if (input.value) input.value.value = ''
}

async function restore() {
  busy.value = true
  try {
    await confirmRestore()
    preview.value = undefined
    restarting.value = true
  } catch (e) {
    error.value = message(e)
    busy.value = false
  }
}
</script>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.progress {
  display: grid;
  gap: 0.25rem;
}

.progress p {
  margin: 0;
}

.preview {
  margin: 0;
  overflow-wrap: anywhere;
}
</style>
