<template>
  <!-- 頻道點數 › 獎勵: the channel point rewards on Twitch. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <div class="rewards">
      <section class="tbl-wrap" aria-labelledby="rw-heading">
        <div class="tbl-top">
          <h3 id="rw-heading" class="tbl-head">{{ t('rewards.heading') }}</h3>
          <Button :label="t('rewards.add')" icon="pi pi-plus" disabled />
        </div>
        <p class="empty-cell">{{ t('rewards.empty') }}</p>
      </section>
      <p class="note">{{ t('rewards.note') }}</p>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const { t } = useI18n()
</script>

<style scoped>
.rewards {
  max-width: 72rem;
}

.note {
  margin: 0.75rem 0 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
