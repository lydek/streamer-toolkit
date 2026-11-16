<template>
  <div class="stack">
    <Button :label="t('backup.export')" icon="pi pi-download" :disabled="busy" @click="exportBackup" />
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import { saveBackup } from './backup'
import { errorToast, successToast } from './home'

const { t } = useI18n()
const toast = useToast()
const busy = ref(false)

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))

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
</script>
