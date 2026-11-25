<template>
  <!-- SO: shoutouts that play the other channel's clip on screen. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('shoutouts.summary')">
    <div class="shoutouts">
      <section class="tbl-wrap" aria-labelledby="so-player">
        <h3 id="so-player" class="tbl-head">{{ t('shoutouts.player') }}</h3>
        <p class="empty-cell">{{ t('shoutouts.nothingPlaying') }}</p>
      </section>
      <section class="tbl-wrap" aria-labelledby="so-session">
        <h3 id="so-session" class="tbl-head">{{ t('shoutouts.session') }}</h3>
        <p class="empty-cell">{{ t('shoutouts.none') }}</p>
      </section>
      <FormPanel :heading="t('shoutouts.settings')">
        <FormRow :label="t('shoutouts.onRaid')" label-id="so-on-raid" end>
          <ToggleSwitch disabled aria-labelledby="so-on-raid" />
        </FormRow>
        <FormRow :label="t('shoutouts.longest')" input-id="so-longest">
          <span class="unit-field">
            <InputNumber v-model="longest" input-id="so-longest" :min="5" :max="60" :use-grouping="false" />
            <span>{{ t('shoutouts.seconds') }}</span>
          </span>
        </FormRow>
        <FormRow :label="t('shoutouts.recent')" input-id="so-recent">
          <span class="unit-field">
            <InputNumber v-model="recent" input-id="so-recent" :min="1" :max="365" :use-grouping="false" />
            <span>{{ t('shoutouts.days') }}</span>
          </span>
        </FormRow>
        <template #foot>
          <span class="note">{{ t('shoutouts.commands') }}</span>
        </template>
      </FormPanel>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import InputNumber from 'primevue/inputnumber'
import ToggleSwitch from 'primevue/toggleswitch'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const { t } = useI18n()
const longest = ref(60)
const recent = ref(90)
</script>

<style scoped>
.shoutouts {
  display: grid;
  gap: 1rem;
  max-width: 72rem;
}

.shoutouts .tbl-wrap {
  margin-top: 0;
}

.unit-field {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}

.unit-field :deep(.p-inputnumber-input) {
  width: 5rem;
}

.note {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
