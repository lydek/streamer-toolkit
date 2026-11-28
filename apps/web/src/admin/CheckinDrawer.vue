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
      <p v-if="today.state === 'failed'" role="alert">{{ t('checkin.page.loadFailed') }}</p>
      <p v-else-if="today.state === 'loading'">{{ t('app.loading') }}</p>
      <template v-else-if="today.state === 'ready'">
        <p>{{ t('checkin.today', { n: today.today.count }) }}</p>
        <ul v-if="today.today.recent.length" class="recent-checkins">
          <li v-for="r in today.today.recent" :key="r.viewer_id">
            <time :datetime="r.occurred_at">{{ formatTime(r.occurred_at) }}</time> {{ r.user_name }}
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
import { useHomeDrawer } from './homeDrawer'
import ObsStatusLink from './ObsStatusLink.vue'
import { simulateCheckin } from './simulate'

defineProps<{ title: string; today: { state: 'loading' | 'failed' } | { state: 'ready'; today: Schemas['CheckinToday'] } }>()
const { t, locale } = useI18n()
const { busy, run, notify } = useHomeDrawer()
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
  list-style: none;
  padding: 0;
  margin: 0;
}
</style>
