<template>
  <!-- 聊天機器人 › 自動感謝與歡迎: one message per event. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('bot.summary')">
    <FormPanel class="greetings">
      <FormRow
        v-for="g in GREETINGS"
        :key="g"
        :label="name(g)"
        :input-id="g === 'bits' ? undefined : `greet-${g}`"
        :label-id="g === 'bits' ? 'greet-bits' : undefined"
        top
      >
        <div class="greet">
          <div class="fields">
            <div v-if="g === 'bits'" class="tiers" role="group" aria-labelledby="greet-bits">
              <label v-for="tier in TIERS" :key="tier" class="tier">
                <span>{{ t(`bot.greetings.bits.tiers.${tier}`) }}</span>
                <InputText :placeholder="t(`bot.greetings.bits.examples.${tier}`)" />
              </label>
            </div>
            <InputText v-else :id="`greet-${g}`" :placeholder="t(`bot.greetings.${g}.example`)" />
            <template v-if="g === 'returning'">
              <SelectButton
                v-model="returning"
                :options="RETURNING"
                :option-label="(m: string) => t(`bot.greetings.returning.modes.${m}`)"
                :allow-empty="false"
                :aria-label="t('bot.greetings.returning.how')"
              />
              <label class="days">
                {{ t('bot.greetings.returning.after') }}
                <InputNumber v-model="days" :min="1" :max="365" :use-grouping="false" input-class="days-input" />
                {{ t('bot.greetings.returning.days') }}
              </label>
            </template>
          </div>
          <ToggleSwitch disabled :aria-label="t('bot.greetings.enabled', { name: name(g) })" />
        </div>
        <template v-if="g === 'returning'" #hint>{{ t('bot.greetings.returning.hint') }}</template>
      </FormRow>
    </FormPanel>
  </FullPage>
</template>

<script setup lang="ts">
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import SelectButton from 'primevue/selectbutton'
import ToggleSwitch from 'primevue/toggleswitch'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const GREETINGS = ['follow', 'firstChat', 'returning', 'sub', 'bits', 'raid'] as const
const TIERS = ['small', 'mid', 'big'] as const
const RETURNING = ['mark', 'say']

const { t } = useI18n()
const name = (g: string) => t(`bot.greetings.${g}.name`)
const returning = ref('mark')
const days = ref(7)
</script>

<style scoped>
.greetings {
  max-width: 60rem;
}

.greet {
  display: flex;
  align-items: flex-start;
  gap: 1rem;
}

.fields {
  display: grid;
  flex: 1;
  gap: 0.5rem;
  justify-items: start;
}

.fields > .p-inputtext,
.tier > .p-inputtext {
  width: 100%;
}

.tiers {
  display: grid;
  gap: 0.5rem;
  width: 100%;
}

.tier {
  display: grid;
  grid-template-columns: 6rem minmax(0, 1fr);
  align-items: center;
  gap: 0.75rem;
  width: 100%;
  color: var(--p-text-muted-color);
}

.days {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}

.days :deep(.days-input) {
  width: 4rem;
}
</style>
