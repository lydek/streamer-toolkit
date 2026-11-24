<template>
  <!-- 訂閱馬拉松: the marathon timer, with its own overlay. -->
  <FullPage tile="counter" overlay="marathon">
    <MarathonTimer :event="live.marathon" @error="report" />
  </FullPage>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { errorMessage } from '../i18n'
import { ApiError } from './api'
import FullPage from './FullPage.vue'
import { errorToast, useLiveEvents } from './home'
import MarathonTimer from './MarathonTimer.vue'

const toast = useToast()
const live = useLiveEvents()
const report = (e: unknown) =>
  toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
</script>
