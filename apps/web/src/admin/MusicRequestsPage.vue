<template>
  <!-- YouTube 點播: viewers queue YouTube songs with !play. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('musicRequests.summary')">
    <div class="requests">
      <div class="bar">
        <SelectButton
          v-model="mode"
          :options="MODES"
          :option-label="(m: string) => t(`musicRequests.modes.${m}`)"
          :allow-empty="false"
          :aria-label="t('musicRequests.mode')"
        />
        <span class="check">
          <Checkbox input-id="mr-open" binary disabled />
          <label for="mr-open">{{ t('musicRequests.open') }}</label>
        </span>
      </div>
      <div class="tbl-wrap">
        <p class="empty-cell">{{ t('musicRequests.empty') }}</p>
        <div class="tbl-foot">
          <span class="note">{{ t('musicRequests.rules') }}</span>
          <Button :label="t('musicRequests.settings')" severity="secondary" disabled />
        </div>
      </div>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import SelectButton from 'primevue/selectbutton'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const MODES = ['audio', 'video']

const { t } = useI18n()
const mode = ref('audio')
</script>

<style scoped>
.requests {
  max-width: 72rem;
}

.bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.75rem 1.5rem;
}

.check {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}
</style>
