<template>
  <!-- 字幕: live captions of what the streamer says, on screen. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('captions.summary')">
    <FormPanel class="captions">
      <FormRow :label="t('captions.on')" label-id="cap-on" end>
        <ToggleSwitch disabled aria-labelledby="cap-on" />
        <template #hint>{{ t('captions.onHint') }}</template>
      </FormRow>
      <FormRow :label="t('captions.mic')" input-id="cap-mic">
        <Select input-id="cap-mic" :options="[]" :placeholder="t('captions.noMic')" disabled />
      </FormRow>
      <FormRow :label="t('captions.language')" input-id="cap-language">
        <Select v-model="language" input-id="cap-language" :options="LANGUAGES" />
      </FormRow>
      <FormRow :label="t('captions.offWhenSinging')" label-id="cap-singing" end>
        <ToggleSwitch disabled aria-labelledby="cap-singing" />
      </FormRow>
    </FormPanel>
  </FullPage>
</template>

<script setup lang="ts">
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const LANGUAGES = ['中文（台灣）', 'English', '日本語']

const { t } = useI18n()
const language = ref(LANGUAGES[0])
</script>

<style scoped>
.captions {
  max-width: 60rem;
}
</style>
