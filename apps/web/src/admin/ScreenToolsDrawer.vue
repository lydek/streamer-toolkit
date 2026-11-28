<template>
  <AppDrawer id="screen-tools" :title="title" full-page="/screen-tools">
    <template #actions>
      <ObsStatusLink overlay="/overlay/break/" />
      <DrawerNotice />
    </template>
    <template #now>
      <p v-if="!live.breakCard">{{ t('app.loading') }}</p>
      <template v-else>
        <p>{{ t(live.breakCard.shown ? 'screenTools.break.shown' : 'screenTools.break.hidden') }}</p>
        <div class="break-quick">
          <Button
            v-for="m in [5, 10, 15]"
            :key="m"
            :label="t('screenTools.break.forMinutes', { n: m })"
            severity="secondary"
            @click="showBreak({ minutes: m }).catch(failed)"
          />
          <Button :label="t('screenTools.break.hide')" text :disabled="!live.breakCard.shown" @click="hideBreak().catch(failed)" />
        </div>
      </template>
      <h3>{{ t('screenTools.warnings.heading') }}</h3>
      <WarningSettings :warnings="live.warnings" :inputs="live.obsState?.inputs ?? []" />
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import type { LiveState } from './home'
import { useHomeDrawer } from './homeDrawer'
import ObsStatusLink from './ObsStatusLink.vue'
import { hideBreak, showBreak } from './screenTools'
import WarningSettings from './WarningSettings.vue'

defineProps<{ title: string; live: LiveState }>()
const { t } = useI18n()
const { failed } = useHomeDrawer()
</script>

<style scoped>
.break-quick {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
</style>
