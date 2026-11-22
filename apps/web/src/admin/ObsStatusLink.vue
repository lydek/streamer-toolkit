<template>
  <!-- Where this overlay is in OBS, linking to the OBS sources page that adds or removes it. -->
  <RouterLink to="/obs-sources" class="obs-status" :class="presence.kind">
    <i :class="ICONS[presence.kind]" aria-hidden="true"></i>{{ text }}
  </RouterLink>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { obsPresence, useObsSources } from './obsSources'

/** The overlay's path, e.g. `/overlay/chat/`; a leaderboard's carries its board and period. */
const props = defineProps<{ overlay: string }>()
const { t } = useI18n()
const data = useObsSources()

const ICONS = {
  offline: 'pi pi-circle',
  missing: 'pi pi-plus-circle',
  duplicate: 'pi pi-exclamation-triangle',
  in: 'pi pi-check-circle',
} as const

const presence = computed(() => obsPresence(props.overlay, data.value))
const text = computed(() => {
  const p = presence.value
  if (p.kind === 'duplicate') return t('obsStatus.duplicate', { n: p.sources })
  if (p.kind === 'in') return t('obsStatus.in', { n: p.scenes, live: t(p.live ? 'obsStatus.withLive' : 'obsStatus.withoutLive') })
  return t(`obsStatus.${p.kind}`)
})
</script>

<style scoped>
.obs-status {
  display: inline-flex;
  gap: 0.375rem;
  align-items: center;
  padding: 0.25rem 0.625rem;
  border: 1px solid var(--p-content-border-color);
  border-radius: 999px;
  color: var(--p-text-color);
  font-size: 0.875rem;
  text-decoration: none;
}

.obs-status:hover {
  border-color: var(--p-primary-color);
}

.in i {
  color: var(--p-green-400);
}

.duplicate {
  border-color: var(--p-yellow-700);
}

.duplicate i,
.missing i {
  color: var(--p-yellow-400);
}

.offline i {
  color: var(--p-text-muted-color);
}
</style>
