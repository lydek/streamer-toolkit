<template>
  <AppDrawer id="alerts" :title="title" full-page="/alerts">
    <template #actions>
      <!-- The latest alerts first, as the prototype; replay works as on 最近事件. -->
      <RecentAlerts compact :notify="notify" />
      <ObsStatusLink overlay="/overlay/alerts/" />
      <form @submit.prevent="sendAlert">
        <p>{{ t('simulate.hint') }}</p>
        <span id="sim-kind">{{ t('simulate.kind') }}</span>
        <SelectButton
          v-model="simAlert.kind"
          :options="ALERT_KINDS"
          :option-label="(k: AlertKind) => t(`alerts.kinds.${k}`)"
          :allow-empty="false"
          aria-labelledby="sim-kind"
        />
        <label for="sim-alert-user">{{ t('simulate.userName') }}</label>
        <InputText id="sim-alert-user" v-model="simAlert.userName" autocomplete="off" maxlength="25" />
        <template v-if="simAlert.kind in AMOUNT_MAX">
          <label for="sim-amount">{{ t(`alerts.amount.${simAlert.kind}`) }}</label>
          <InputNumber
            v-model="simAlert.amount"
            input-id="sim-amount"
            :min="1"
            :max="AMOUNT_MAX[simAlert.kind]"
            :use-grouping="false"
            required
          />
        </template>
        <template v-if="simAlert.kind === 'resubscribe' || simAlert.kind === 'cheer'">
          <label for="sim-alert-message">{{ t('simulate.message') }}</label>
          <InputText id="sim-alert-message" v-model="simAlert.message" autocomplete="off" maxlength="500" />
        </template>
        <Button type="submit" :label="t('simulate.sendAlert')" icon="pi pi-send" :disabled="busy" />
      </form>
      <DrawerNotice />
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import SelectButton from 'primevue/selectbutton'
import { reactive } from 'vue'
import { useI18n } from 'vue-i18n'
import { ALERT_KINDS, type AlertKind } from '../overlay/alerts/alerts'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import { useHomeDrawer } from './homeDrawer'
import ObsStatusLink from './ObsStatusLink.vue'
import RecentAlerts from './RecentAlerts.vue'
import { simulateAlert } from './simulate'

defineProps<{ title: string }>()
const { t } = useI18n()
const { busy, run, notify } = useHomeDrawer()
// The Host's limits for the kinds that take an amount (crates/host/src/admin.rs).
const AMOUNT_MAX: Partial<Record<AlertKind, number>> = { resubscribe: 100000, gift: 100, cheer: 100000, raid: 100000 }
const simAlert = reactive({ kind: 'follow' as AlertKind, userName: '', amount: 5 as number | null, message: '' })

const sendAlert = () =>
  run(async () => {
    const { kind, userName, amount, message } = simAlert
    await simulateAlert({ kind, userName, amount, message })
    notify(t('simulate.alertSent'), false)
  })
</script>
