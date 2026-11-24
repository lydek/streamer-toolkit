<template>
  <!-- 打卡規則: how check-ins work (command, reward, slots, time zone, day change) and their sounds. -->
  <FullPage tile="checkin" overlay="checkin">
    <CheckinSettings @error="report" />
    <SoundSlots
      :slots="[
        { slot: 'checkin', label: t('sound.checkinNormal') },
        { slot: 'checkin.full', label: t('sound.checkinFull') },
      ]"
    />
  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import CheckinSettings from './CheckinSettings.vue'
import FullPage from './FullPage.vue'
import { errorToast } from './home'
import SoundSlots from './SoundSlots.vue'

const { t } = useI18n()
const toast = useToast()
const report = (e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
</script>
