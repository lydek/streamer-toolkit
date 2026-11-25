<template>
  <!-- 虛擬形象 › 跳跳: the mic avatar that swaps images for talking, idle and AFK. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('avatar.mic.summary')">
    <div class="mic-avatar">
      <FormPanel>
        <FormRow :label="t('avatar.mic.device')" input-id="av-mic">
          <Select input-id="av-mic" :options="[]" :placeholder="t('avatar.mic.noDevice')" disabled />
        </FormRow>
        <FormRow :label="t('avatar.mic.level')" label-id="av-level">
          <!-- No mic yet: the bar stays empty; the line marks the speaking threshold. -->
          <span class="meter" role="img" :aria-label="t('avatar.mic.levelNone')">
            <b :style="{ left: `${threshold}%` }"></b>
          </span>
        </FormRow>
        <FormRow :label="t('avatar.mic.threshold')" input-id="av-threshold">
          <span class="range">
            <input id="av-threshold" v-model.number="threshold" type="range" min="0" max="100" />
            <output for="av-threshold">{{ threshold }}%</output>
          </span>
        </FormRow>
        <FormRow :label="t('avatar.mic.highPass')" label-id="av-high-pass" end>
          <ToggleSwitch disabled aria-labelledby="av-high-pass" />
        </FormRow>
        <FormRow :label="t('avatar.mic.smoothing')" input-id="av-smoothing">
          <span class="range">
            <input id="av-smoothing" v-model.number="smoothing" type="range" min="0" max="100" />
            <output for="av-smoothing">{{ smoothing }}%</output>
          </span>
        </FormRow>
        <FormRow :label="t('avatar.mic.afk')" input-id="av-afk">
          <span class="unit-field">
            <InputNumber v-model="afk" input-id="av-afk" :min="0" :max="60" :use-grouping="false" />
            <span>{{ t('avatar.mic.afkUnit') }}</span>
          </span>
        </FormRow>
        <FormRow :label="t('avatar.mic.animation')" input-id="av-animation">
          <Select
            v-model="animation"
            input-id="av-animation"
            :options="ANIMATIONS"
            :option-label="(a: string) => t(`avatar.mic.animations.${a}`)"
          />
        </FormRow>
      </FormPanel>
      <section class="tbl-wrap" aria-labelledby="av-sets">
        <div class="tbl-top">
          <h3 id="av-sets" class="tbl-head">{{ t('avatar.sets') }}</h3>
          <Button :label="t('avatar.addSet')" icon="pi pi-plus" severity="secondary" size="small" disabled />
        </div>
        <p class="empty-cell">{{ t('avatar.noSets') }}</p>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const ANIMATIONS = ['bounce', 'fade', 'zoom', 'slide', 'cut']

const { t } = useI18n()
const threshold = ref(30)
const smoothing = ref(60)
const afk = ref(3)
const animation = ref('bounce')
</script>

<style scoped>
.mic-avatar {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(22rem, 100%), 1fr));
  gap: 1rem;
  align-items: start;
  max-width: 90rem;
}

.mic-avatar .tbl-wrap {
  margin-top: 0;
}

.meter {
  position: relative;
  display: block;
  width: min(14rem, 100%);
  height: 0.5rem;
  border-radius: 999px;
  background: var(--p-surface-800);
}

.meter b {
  position: absolute;
  top: -0.25rem;
  bottom: -0.25rem;
  width: 2px;
  background: var(--p-text-muted-color);
}

.range,
.unit-field {
  display: inline-flex;
  align-items: center;
  gap: 0.75rem;
}

.unit-field :deep(.p-inputnumber-input) {
  width: 5rem;
}

.range input {
  width: min(14rem, 100%);
}
</style>
