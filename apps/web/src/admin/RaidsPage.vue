<template>
  <!-- 揪團: start a raid from the toolkit. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('raids.summary')">
    <div class="raids">
      <div class="split">
        <FormPanel :heading="t('raids.to')">
          <FormRow :label="t('raids.channel')" input-id="raid-channel">
            <InputText id="raid-channel" :placeholder="t('raids.channelHint')" />
          </FormRow>
          <FormRow :label="t('raids.before')" label-id="raid-before" top>
            <div class="checks" role="group" aria-labelledby="raid-before">
              <span v-for="b in BEFORE" :key="b" class="check">
                <Checkbox v-model="before" :input-id="`raid-${b}`" :value="b" />
                <label :for="`raid-${b}`">{{ t(`raids.beforeItems.${b}`) }}</label>
              </span>
            </div>
          </FormRow>
          <template #foot>
            <Button :label="t('raids.cancel')" severity="secondary" disabled />
            <Button :label="t('raids.start')" disabled />
          </template>
        </FormPanel>
        <section class="tbl-wrap" aria-labelledby="raid-random">
          <div class="tbl-top">
            <h3 id="raid-random" class="tbl-head">{{ t('raids.random') }}</h3>
            <Button :label="t('raids.pick')" severity="secondary" disabled />
          </div>
          <p class="empty-cell">{{ t('raids.randomHint') }}</p>
        </section>
      </div>
      <p class="note">{{ t('raids.scopes') }}</p>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const BEFORE = ['credits', 'announce']

const { t } = useI18n()
const before = ref([...BEFORE])
</script>

<style scoped>
.raids {
  max-width: 72rem;
}

.split {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(22rem, 100%), 1fr));
  gap: 1rem;
  align-items: start;
}

.split .tbl-wrap {
  margin-top: 0;
}

.checks {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1.25rem;
}

.check {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}

.note {
  margin: 0.75rem 0 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
