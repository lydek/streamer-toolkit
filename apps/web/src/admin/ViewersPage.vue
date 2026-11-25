<template>
  <!-- 觀眾清單: every viewer with their roles and totals. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <template #actions>
      <Button :label="t('viewers.history')" icon="pi pi-history" severity="secondary" disabled />
      <Button :label="t('viewers.sync')" icon="pi pi-refresh" disabled />
    </template>
    <form class="filters" :aria-label="t('viewers.search')" @submit.prevent>
      <div class="field grow">
        <label for="vw-name">{{ t('viewers.viewer') }}</label>
        <InputText id="vw-name" :placeholder="t('viewers.viewerHint')" autocomplete="off" />
      </div>
      <div class="field">
        <label for="vw-role">{{ t('viewers.role') }}</label>
        <Select v-model="role" input-id="vw-role" :options="ROLES" :option-label="(r: string) => t(`viewers.roles.${r}`)" />
      </div>
      <div class="buttons">
        <Button type="submit" :label="t('viewers.searchButton')" icon="pi pi-search" disabled />
      </div>
    </form>
    <div class="tbl-wrap">
      <p class="empty-cell">{{ t('viewers.empty') }}</p>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const ROLES = ['all', 'subscriber', 'moderator', 'vip']

const { t } = useI18n()
const role = ref('all')
</script>
