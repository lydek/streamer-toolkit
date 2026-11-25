<template>
  <!-- 投票與預測: Twitch's own polls and predictions. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('polls.summary')">
    <div class="polls">
      <div class="current">
        <section v-for="k in KINDS" :key="k" class="tbl-wrap" :aria-labelledby="`polls-${k}`">
          <h3 :id="`polls-${k}`" class="tbl-head">{{ t(`polls.kinds.${k}`) }}</h3>
          <p class="empty-cell">{{ t(`polls.none.${k}`) }}</p>
        </section>
      </div>

      <FormPanel :heading="t('polls.new')">
        <FormRow :label="t('polls.kind')" label-id="polls-kind">
          <SelectButton
            v-model="kind"
            :options="KINDS"
            :option-label="(k: Kind) => t(`polls.kinds.${k}`)"
            :allow-empty="false"
            aria-labelledby="polls-kind"
          />
        </FormRow>
        <FormRow :label="t('polls.question')" input-id="polls-question">
          <InputText id="polls-question" class="long" :placeholder="t('polls.questionHint')" />
        </FormRow>
        <FormRow :label="t('polls.options')" label-id="polls-options" top>
          <div class="options" role="group" aria-labelledby="polls-options">
            <InputText
              v-for="n in options"
              :key="n"
              :placeholder="t('polls.option', { n })"
              :aria-label="t('polls.option', { n })"
            />
            <Button :label="t('polls.addOption')" icon="pi pi-plus" text :disabled="options >= 5" @click="options++" />
          </div>
        </FormRow>
        <FormRow :label="t('polls.duration')" input-id="polls-duration">
          <Select
            v-model="minutes"
            input-id="polls-duration"
            :options="DURATIONS.map((n) => ({ label: t('polls.minutes', { n }), value: n }))"
            option-label="label"
            option-value="value"
          />
        </FormRow>
        <template #foot>
          <span class="note">{{ t('polls.saveHint') }}</span>
          <Button :label="t('polls.start')" disabled />
        </template>
      </FormPanel>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import SelectButton from 'primevue/selectbutton'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

type Kind = 'poll' | 'prediction'
const KINDS: Kind[] = ['poll', 'prediction']
const DURATIONS = [1, 2, 5]

const { t } = useI18n()
const kind = ref<Kind>('poll')
const options = ref(2)
const minutes = ref(2)
</script>

<style scoped>
.polls {
  display: grid;
  gap: 1rem;
  max-width: 72rem;
}

.current {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(18rem, 100%), 1fr));
  gap: 1rem;
}

.current .tbl-wrap {
  margin-top: 0;
}

.long,
.options > .p-inputtext {
  width: 20rem;
  max-width: 100%;
}

.options {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 0.5rem;
  justify-items: start;
}


.note {
  margin-right: auto;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
