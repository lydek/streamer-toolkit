<template>
  <div class="stack">
    <Button :label="t('backup.export')" icon="pi pi-download" :disabled="busy" @click="exportBackup" />
    <label for="restore-file">{{ t('backup.restoreFile') }}</label>
    <input id="restore-file" ref="input" type="file" accept=".stbackup" :disabled="busy" @change="choose" />
    <p v-if="error" role="alert">{{ error }}</p>
    <template v-if="preview">
      <ul class="preview">
        <li>
          {{ t('backup.createdAt') }}<time :datetime="preview.created_at">{{ formatTime(preview.created_at) }}</time>
        </li>
        <li>{{ t('backup.appVersion', { version: preview.app_version }) }}</li>
        <li>{{ t('backup.records', { redemptions: preview.redemptions, checkins: preview.checkins }) }}</li>
        <li>{{ t('backup.files', { templates: preview.templates, sounds: preview.sounds, music: preview.music, media: preview.media }) }}</li>
      </ul>
      <p>{{ t('backup.restoreNote') }}</p>
      <Button :label="t('backup.restore')" icon="pi pi-replay" :disabled="busy" @click="restore" />
    </template>
    <p v-if="restarting" role="status">{{ t('backup.restarting') }}</p>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas } from './api'
import { BACKUP_MAX_BYTES, confirmRestore, previewRestore, saveBackup } from './backup'
import { errorToast, successToast } from './home'

const { t, locale } = useI18n()
const toast = useToast()
const busy = ref(false)
const input = ref<HTMLInputElement>()
const preview = shallowRef<Schemas['RestorePreview']>()
const error = ref('')
const restarting = ref(false)

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))
const formatTime = (iso: string) =>
  new Date(iso).toLocaleString(locale.value, { dateStyle: 'medium', timeStyle: 'short' })

async function exportBackup() {
  busy.value = true
  try {
    if (await saveBackup()) toast.add(successToast(t('backup.exported')))
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
  if (file.size > BACKUP_MAX_BYTES) return refuse(t('errors.backup.too_large'))
  busy.value = true
  try {
    preview.value = await previewRestore(file)
  } catch (e) {
    refuse(message(e))
  } finally {
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
.preview {
  margin: 0;
  overflow-wrap: anywhere;
}
</style>
