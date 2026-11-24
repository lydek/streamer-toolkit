<template>
  <!-- How every built-in overlay looks (theme, text size), chosen apart from the admin UI's look. -->
  <FullPage>
    <section class="stack" aria-labelledby="appearance-heading">
      <h2 id="appearance-heading">{{ t('screenTools.appearance.heading') }}</h2>
      <AppearanceSettings :appearance="live.appearance" />
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppearanceSettings from './AppearanceSettings.vue'
import FullPage from './FullPage.vue'
import { useLiveEvents } from './home'

const { t } = useI18n()
const live = useLiveEvents()
</script>
