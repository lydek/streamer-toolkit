<template>
  <AppDrawer id="checkin" :title="title" full-page="/checkin">
    <template #actions>
      <ObsStatusLink overlay="/overlay/checkin/" />
      <form @submit.prevent="sendCheckin">
        <p>{{ t('simulate.hint') }}</p>
        <label for="sim-checkin-user">{{ t('simulate.userName') }}</label>
        <InputText id="sim-checkin-user" v-model="simCheckin.userName" autocomplete="off" maxlength="25" />
        <label for="sim-checkin-stamps">{{ t('simulate.stamps') }}</label>
        <InputNumber
          v-model="simCheckin.stamps"
          input-id="sim-checkin-stamps"
          :min="1"
          :max="100"
          :use-grouping="false"
          required
        />
        <Button type="submit" :label="t('simulate.sendCheckin')" icon="pi pi-send" :disabled="busy" />
      </form>
      <DrawerNotice />
    </template>
    <template #now>
      <!-- Redemptions that kept failing, with retry and skip as in 設定 › 功能 › 打卡; nothing while there are none. -->
      <CheckinOutbox :state="undefined" @error="failed" />
      <p v-if="today.state === 'failed'" role="alert">{{ t('checkin.page.loadFailed') }}</p>
      <p v-else-if="today.state === 'loading'">{{ t('app.loading') }}</p>
      <template v-else-if="today.state === 'ready'">
        <p>{{ t('checkin.today', { n: today.today.count }) }}</p>
        <ul v-if="today.today.recent.length" class="recent-checkins">
          <li v-for="r in today.today.recent" :key="r.viewer_id">
            <span class="avatar" aria-hidden="true">{{ Array.from(r.user_name)[0] ?? '' }}</span>
            <span class="who">{{ r.user_name }}</span>
            <time :datetime="r.occurred_at">{{ formatTime(r.occurred_at) }}</time>
          </li>
        </ul>
      </template>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import { reactive } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Schemas } from './api'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import CheckinOutbox from './CheckinOutbox.vue'
import { useHomeDrawer } from './homeDrawer'
import ObsStatusLink from './ObsStatusLink.vue'
import { simulateCheckin } from './simulate'

defineProps<{ title: string; today: { state: 'loading' | 'failed' } | { state: 'ready'; today: Schemas['CheckinToday'] } }>()
const { t, locale } = useI18n()
const { busy, run, notify, failed } = useHomeDrawer()
const simCheckin = reactive({ userName: '', stamps: 3 as number | null })
const formatTime = (iso: string) => new Date(iso).toLocaleTimeString(locale.value, { timeStyle: 'short' })

const sendCheckin = () =>
  run(async () => {
    const { userName, stamps } = simCheckin
    await simulateCheckin({ userName, stamps: stamps ?? 0 })
    notify(t('simulate.checkinSent'), false)
  })
</script>

<style scoped>
.recent-checkins {
  display: grid;
  gap: 0.375rem;
  list-style: none;
  padding: 0;
  margin: 0;
}

.recent-checkins li {
  display: flex;
  align-items: center;
  gap: 0.625rem;
}

.avatar {
  display: inline-grid;
  flex: none;
  place-items: center;
  width: 1.75rem;
  height: 1.75rem;
  border-radius: 50%;
  background: var(--p-surface-700);
  font-size: 0.8125rem;
}

.who {
  min-width: 0;
  margin-right: auto;
  overflow-wrap: anywhere;
}

time {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}
</style>
