<template>
  <AppDrawer id="counter" :title="title" full-page="/counters">
    <template #actions>
      <ObsStatusLink overlay="/overlay/counter/" />
      <DrawerNotice />
    </template>
    <template #now>
      <p v-if="!counters">{{ t('app.loading') }}</p>
      <p v-else-if="!counters.length">{{ t('counter.empty') }}</p>
      <ul v-else class="counter-list">
        <li v-for="c in counters" :key="c.id">
          <span>{{ c.name }}</span>
          <output class="counter-value">{{ c.value }}</output>
          <Button icon="pi pi-minus" :aria-label="t('counter.minusOneFor', { name: c.name })" outlined @click="stepCounter(c.id, -1).catch(failed)" />
          <Button icon="pi pi-plus" :aria-label="`${t('counter.plusOne')}：${c.name}`" outlined @click="stepCounter(c.id, 1).catch(failed)" />
        </li>
      </ul>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import { stepCounter } from './counters'
import DrawerNotice from './DrawerNotice.vue'
import type { LiveState } from './home'
import { useHomeDrawer } from './homeDrawer'
import ObsStatusLink from './ObsStatusLink.vue'

defineProps<{ title: string; counters?: LiveState['counters'] }>()
const { t } = useI18n()
const { failed } = useHomeDrawer()
</script>

<style scoped>
.counter-list {
  padding: 0;
  list-style: none;
}

.counter-list li {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.counter-value {
  margin-inline-start: auto;
  font-size: 1.25rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
</style>
