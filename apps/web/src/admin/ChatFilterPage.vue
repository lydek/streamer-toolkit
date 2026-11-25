<template>
  <!-- 聊天過濾: rules that delete, time out or ban, and what they caught. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <div class="chat-filter">
      <div class="split">
        <FormPanel :heading="t('chatFilter.rules')">
          <FormRow v-for="r in RULES" :key="r" :label="t(`chatFilter.rule.${r}.name`)" :input-id="`cf-${r}`">
            <div class="rule">
              <Select
                v-model="actions[r]"
                :input-id="`cf-${r}`"
                :options="ACTIONS"
                :option-label="(a: string) => t(`chatFilter.actions.${a}`)"
              />
              <ToggleSwitch disabled :aria-label="t('chatFilter.enabled', { name: t(`chatFilter.rule.${r}.name`) })" />
            </div>
            <template #hint>{{ t(`chatFilter.rule.${r}.hint`) }}</template>
          </FormRow>
          <FormRow :label="t('chatFilter.exempt')" label-id="cf-exempt">
            <span class="with-button">
              <span>{{ t('chatFilter.exemptWho') }}</span>
              <Button :label="t('chatFilter.change')" text size="small" disabled />
            </span>
          </FormRow>
          <FormRow :label="t('chatFilter.automod')" label-id="cf-automod">
            <span class="with-button">
              <span>{{ t('chatFilter.automodWhere') }}</span>
              <Button :label="t('chatFilter.open')" text size="small" disabled />
            </span>
          </FormRow>
        </FormPanel>

        <div class="side">
          <section class="tbl-wrap" aria-labelledby="cf-terms">
            <div class="tbl-top">
              <h3 id="cf-terms" class="tbl-head">{{ t('chatFilter.terms') }}</h3>
              <Button :label="t('chatFilter.add')" icon="pi pi-plus" severity="secondary" disabled />
            </div>
            <p class="empty-cell">{{ t('chatFilter.noTerms') }}</p>
          </section>
          <section class="tbl-wrap" aria-labelledby="cf-urls">
            <h3 id="cf-urls" class="tbl-head">{{ t('chatFilter.urls') }}</h3>
            <p class="empty-cell">{{ t('chatFilter.noUrls') }}</p>
            <div class="tbl-foot">
              <InputText :placeholder="t('chatFilter.urlHint')" :aria-label="t('chatFilter.addUrl')" />
              <Button :label="t('chatFilter.addUrlButton')" severity="secondary" disabled />
            </div>
          </section>
          <p class="note">{{ t('chatFilter.urlNote') }}</p>
        </div>
      </div>

      <section class="tbl-wrap" aria-labelledby="cf-recent">
        <h3 id="cf-recent" class="tbl-head">{{ t('chatFilter.recent') }}</h3>
        <p class="empty-cell">{{ t('chatFilter.noRecent') }}</p>
      </section>
      <p class="note">{{ t('chatFilter.scopes') }}</p>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'
import { reactive } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const RULES = ['terms', 'links', 'repeats', 'emotes', 'caps', 'chars'] as const
const ACTIONS = ['delete', 'timeout', 'ban']

const { t } = useI18n()
// The prototype's defaults: spam that repeats is timed out, the rest deleted.
const actions = reactive<Record<string, string>>(Object.fromEntries(RULES.map((r) => [r, r === 'repeats' ? 'timeout' : 'delete'])))
</script>

<style scoped>
.chat-filter {
  display: grid;
  gap: 1rem;
  max-width: 90rem;
}

.chat-filter .tbl-wrap {
  margin-top: 0;
}

.split {
  display: grid;
  grid-template-columns: minmax(0, 1.4fr) minmax(min(20rem, 100%), 1fr);
  gap: 1rem;
  align-items: start;
}

.side {
  display: grid;
  gap: 1rem;
  min-width: 0;
}

.rule,
.with-button {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem 1rem;
}

.with-button {
  justify-content: flex-start;
}

.note {
  margin: 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
  /* The scope names are long words with no spaces. */
  overflow-wrap: anywhere;
}

@container view (max-width: 56rem) {
  .split {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
