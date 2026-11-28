<template>
  <AppDrawer id="rehearsal" :title="title" full-page="/rehearsal">
    <template #actions>
      <DrawerNotice />
    </template>
    <template #now>
      <ul class="rehearsal-list">
        <li v-for="id in BUILT_IN" :key="id">
          <span>{{ t(`rehearsal.scripts.${id}`) }}</span>
          <Button :label="t('rehearsal.start')" icon="pi pi-play" size="small" outlined @click="runBuiltIn(id, 'normal').catch(failed)" />
        </li>
      </ul>
      <Button :label="t('rehearsal.stop')" icon="pi pi-stop" severity="danger" text @click="stopScript().catch(() => {})" />
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import DrawerNotice from './DrawerNotice.vue'
import { useHomeDrawer } from './homeDrawer'
import { BUILT_IN, runBuiltIn, stopScript } from './rehearsal'

defineProps<{ title: string }>()
const { t } = useI18n()
const { failed } = useHomeDrawer()
</script>

<style scoped>
.rehearsal-list {
  padding: 0;
  list-style: none;
}

.rehearsal-list li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.25rem 0;
}
</style>
