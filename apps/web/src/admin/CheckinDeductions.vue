<template>
  <section class="stack" aria-labelledby="deductions-heading">
    <h2 id="deductions-heading">{{ t('checkin.deduct.log') }}</h2>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!items">{{ t('app.loading') }}</p>
    <p v-else-if="!items.length">{{ t('checkin.deduct.logEmpty') }}</p>
    <ul v-else class="log">
      <li v-for="d in items" :key="d.id">
        <time :datetime="d.deducted_at">{{ formatTime(d.deducted_at) }}</time>
        <span>{{ t('checkin.deduct.logLine', { name: d.user_name, n: d.amount }) }}</span>
        <span v-if="d.reason" class="reason">{{ d.reason }}</span>
      </li>
    </ul>
  </section>
</template>

<script setup lang="ts">
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'

const { t, locale } = useI18n()
const items = shallowRef<Schemas['CheckinDeduction'][]>()
const failed = ref('')
const formatTime = (iso: string) => new Date(iso).toLocaleString(locale.value, { dateStyle: 'short', timeStyle: 'short' })

async function load() {
  try {
    items.value = await api(client.GET('/api/admin/checkin/deductions'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}

void load()
defineExpose({ load })
</script>

<style scoped>
.log {
  margin: 0;
  padding: 0;
  list-style: none;
}

.log li {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem 1rem;
  padding: 0.375rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}

.reason {
  color: var(--p-text-muted-color);
  overflow-wrap: anywhere;
}
</style>
