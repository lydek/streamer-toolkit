<template>
  <!-- 虛擬形象 › 換裝: viewers pick an outfit with a reward, switched back after a while. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('avatar.outfits.summary')">
    <FormPanel class="outfits">
      <FormRow :label="t('avatar.outfits.list')" label-id="av-outfits">
        <span class="muted">{{ t('avatar.outfits.none') }}</span>
      </FormRow>
      <FormRow :label="t('avatar.outfits.reward')" input-id="av-reward">
        <Select input-id="av-reward" :options="[]" :placeholder="t('avatar.outfits.noReward')" disabled />
      </FormRow>
      <FormRow :label="t('avatar.outfits.back')" input-id="av-back">
        <span class="unit-field">
          <InputNumber v-model="back" input-id="av-back" :min="0" :max="120" :use-grouping="false" />
          <span>{{ t('avatar.outfits.backUnit') }}</span>
        </span>
      </FormRow>
    </FormPanel>
  </FullPage>
</template>

<script setup lang="ts">
import InputNumber from 'primevue/inputnumber'
import Select from 'primevue/select'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const { t } = useI18n()
const back = ref(10)
</script>

<style scoped>
.outfits {
  max-width: 60rem;
}

.muted {
  color: var(--p-text-muted-color);
}

.unit-field {
  display: inline-flex;
  align-items: center;
  gap: 0.75rem;
}

.unit-field :deep(.p-inputnumber-input) {
  width: 5rem;
}
</style>
