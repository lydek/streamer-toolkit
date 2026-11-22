<template>
  <!-- 每種事件: each alert kind's sound, with a test that plays the kind with the current template and sound. -->
  <FullPage :title="t('modules.names.alerts')" tile="alerts" overlay="alerts">
    <SoundSlots built-in :slots="ALERT_KINDS.map((k) => ({ slot: `alert.${k}`, label: t(`alerts.kinds.${k}`) }))">
      <template #actions="{ slotId, label }">
        <Button
          :label="t('alerts.test')"
          icon="pi pi-play"
          text
          :aria-label="t('alerts.testKind', { kind: label })"
          :disabled="testing"
          @click="test(slotId.slice('alert.'.length) as AlertKind)"
        />
      </template>
    </SoundSlots>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ALERT_KINDS, type AlertKind } from '../overlay/alerts/alerts'
import { ApiError } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'
import { ALERT_AMOUNTS, simulateAlert } from './simulate'
import SoundSlots from './SoundSlots.vue'

const { t } = useI18n()
const toast = useToast()
const testing = ref(false)

/** A simulated alert of `kind`: it goes through the presentation queue and is never recorded. */
async function test(kind: AlertKind) {
  testing.value = true
  try {
    await simulateAlert({ kind, amount: ALERT_AMOUNTS[kind] ?? null })
    toast.add(successToast(t('alerts.tested')))
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    testing.value = false
  }
}
</script>
