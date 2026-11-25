<template>
  <!-- 桌面置頂聊天室: the chat in its own always-on-top window. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <template #actions>
      <Button :label="t('chatWindow.open')" icon="pi pi-external-link" disabled />
    </template>
    <FormPanel class="chat-window">
      <FormRow :label="t('chatWindow.opacity')" input-id="cw-opacity">
        <span class="range">
          <input id="cw-opacity" v-model.number="opacity" type="range" min="20" max="100" />
          <output for="cw-opacity">{{ opacity }}%</output>
        </span>
      </FormRow>
      <FormRow :label="t('chatWindow.zoom')" input-id="cw-zoom">
        <span class="range">
          <input id="cw-zoom" v-model.number="zoom" type="range" min="50" max="200" step="10" />
          <output for="cw-zoom">{{ zoom }}%</output>
        </span>
      </FormRow>
      <FormRow :label="t('chatWindow.clickThrough')" label-id="cw-through" end>
        <ToggleSwitch disabled aria-labelledby="cw-through" />
        <template #hint>{{ t('chatWindow.clickThroughHint') }}</template>
      </FormRow>
      <FormRow :label="t('chatWindow.unlock')" label-id="cw-unlock">
        <span class="keys">
          <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>L</kbd>
          <Button :label="t('chatWindow.rebind')" text size="small" disabled />
        </span>
      </FormRow>
      <FormRow :label="t('chatWindow.show')" label-id="cw-show" top>
        <div class="checks" role="group" aria-labelledby="cw-show">
          <span v-for="s in SHOW" :key="s" class="check">
            <Checkbox v-model="shown" :input-id="`cw-${s}`" :value="s" />
            <label :for="`cw-${s}`">{{ t(`chatWindow.items.${s}`) }}</label>
          </span>
        </div>
      </FormRow>
    </FormPanel>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import ToggleSwitch from 'primevue/toggleswitch'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const SHOW = ['time', 'firstChat', 'redemptions', 'support']

const { t } = useI18n()
const opacity = ref(70)
const zoom = ref(100)
// The prototype's defaults: everything but subs, bits and raids.
const shown = ref(['time', 'firstChat', 'redemptions'])
</script>

<style scoped>
.chat-window {
  max-width: 60rem;
}

.range,
.keys,
.check {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

.range input {
  width: min(14rem, 100%);
}

.checks {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1.25rem;
}

kbd {
  padding: 0.125rem 0.375rem;
  border: 1px solid var(--p-surface-600);
  border-radius: 4px;
  font: inherit;
  font-size: 0.8125rem;
}
</style>
