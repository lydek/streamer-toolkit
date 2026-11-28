<template>
  <AppDrawer id="image-sizes" :title="title" full-page="/image-sizes">
    <template #now>
      <!-- Straight into the kind wanted, as the prototype's two entries. -->
      <ul class="kinds">
        <li v-for="k in KINDS" :key="k">
          <RouterLink :to="{ path: '/image-sizes', query: { kind: k } }" class="kind">
            <i :class="ICONS[k]" aria-hidden="true"></i>{{ t(`imageSizes.kinds.${k}`) }}
          </RouterLink>
        </li>
      </ul>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import AppDrawer from './AppDrawer.vue'
import type { ImageKind } from './imageSizes'

defineProps<{ title: string }>()
const { t } = useI18n()
const KINDS: ImageKind[] = ['badge', 'points']
const ICONS: Record<ImageKind, string> = { badge: 'pi pi-star', points: 'pi pi-circle' }
</script>

<style scoped>
.kinds {
  display: grid;
  gap: 0.5rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.kind {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.625rem 0.75rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  color: inherit;
  text-decoration: none;
}

.kind:hover {
  background: var(--p-surface-800);
}

.kind i {
  color: var(--p-primary-color);
}
</style>
