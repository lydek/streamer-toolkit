<template>
  <!-- One stage block on home: 開播前檢查 (`checklist`), or the session (`session`: 本場直播 while live, 本場摘要
       after). The stage itself and its data are shared through `stage` (stage.ts). -->
  <section v-if="part === 'checklist'" class="stage" :aria-labelledby="headingId">
    <div class="head">
      <h2 :id="headingId">{{ t('home.blocks.checklist') }}</h2>
      <span class="ready">{{ t('stage.checks.ready', { ok: checks.filter((c) => c.ok).length, all: checks.length }) }}</span>
      <!-- Folded, only this row shows, whatever is still not ready: the count says how many are. -->
      <Button
        :label="t(openChecks ? 'stage.checks.fold' : 'stage.checks.unfold')"
        :icon="openChecks ? 'pi pi-angle-up' : 'pi pi-angle-down'"
        text
        size="small"
        :aria-expanded="openChecks"
        :aria-controls="`${headingId}-list`"
        @click="openChecks = !openChecks"
      />
    </div>
    <ul :id="`${headingId}-list`" class="plain checks" v-show="openChecks">
      <!-- An overlay not in OBS yet only warns: the stream works without it. -->
      <li v-for="c in checks" :key="c.id" :class="c.ok ? 'ok' : c.id.startsWith('overlay.') ? 'warn' : 'bad'">
        <i :class="c.ok ? 'pi pi-check-circle' : c.id.startsWith('overlay.') ? 'pi pi-exclamation-circle' : 'pi pi-times-circle'" aria-hidden="true"></i>
        <span>{{ t(`stage.checks.${c.id}.${c.ok ? 'ok' : 'bad'}`) }}</span>
        <!-- An overlay is fixed on the OBS sources page, which adds it to a scene. -->
        <ObsStatusLink v-if="!c.ok && c.id.startsWith('overlay.')" :overlay="`/overlay/${c.id.slice('overlay.'.length)}/`" class="fix" />
        <RouterLink v-else-if="!c.ok" :to="c.fix" class="fix">{{ t('stage.checks.fix') }}</RouterLink>
      </li>
    </ul>
  </section>
  <section v-else class="stage" :aria-labelledby="headingId">
    <div class="head">
      <h2 :id="headingId">{{ t(shown === 'live' ? 'home.blocks.stats' : 'home.blocks.summary') }}</h2>
      <slot name="actions" />
    </div>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="notLive" class="muted">{{ t('stage.notLive') }}</p>
    <p v-else-if="!session" class="muted">{{ shown === 'live' ? t('stage.waiting') : t('stage.noStream') }}</p>
    <template v-else>
      <!-- The peak viewer count is a number too: masked like the others. -->
      <p v-if="session.peakViewers != null && masked?.('stats.peak')" class="line">
        {{ when }} ·
        <button type="button" class="value masked inline" :aria-label="`${t('home.numbers.reveal')}：${t('stage.stats.peak')}`" @click="emit('reveal', 'stats.peak')">
          {{ t('stage.peak', { n: '––' }) }}
        </button>
      </p>
      <p v-else class="line">{{ line }}</p>
      <ul class="plain stats">
        <li v-for="s in stats" :key="s.key">
          <!-- A masked number (下播後) is a button that shows just that one. -->
          <button
            v-if="masked?.(numberId(s.key))"
            type="button"
            class="value masked"
            :aria-label="`${t('home.numbers.reveal')}：${t(`stage.stats.${s.key}`)}`"
            @click="emit('reveal', numberId(s.key))"
          >
            ––
          </button>
          <span v-else class="value">{{ s.value.toLocaleString('zh-TW') }}</span>
          <span class="label">{{ t(`stage.stats.${s.key}`) }}</span>
        </li>
      </ul>
      <div v-if="session.endedAt" class="export">
        <Button :label="t('record.export')" icon="pi pi-download" outlined :disabled="exporting" @click="exportIt" />
        <!-- After the stream, a way to 設定 › 資料與備份 (its heading takes focus). -->
        <RouterLink to="/settings/backup#backup" class="p-button p-button-outlined backup-link">
          <i class="pi pi-database" aria-hidden="true"></i>{{ t('record.backup') }}
        </RouterLink>
        <p v-if="exportNotice" :role="exportNotice.error ? 'alert' : 'status'">{{ exportNotice.text }}</p>
      </div>
    </template>
  </section>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import type { StageState } from './stage'
import ObsStatusLink from './ObsStatusLink.vue'

/** `masked`: which session numbers show as 「––」 until revealed (`reveal` with the number's ID). */
const props = defineProps<{ part: 'checklist' | 'session'; stage: StageState; masked?: (id: string) => boolean }>()
const emit = defineEmits<{ reveal: [id: string] }>()
const { t } = useI18n()
// The Host keeps revealed IDs in lowercase, digits, `.` and `-`: `chatMessages` is `stats.chat-messages`.
const numberId = (key: string) => `stats.${key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`
// A stage may hold both 本場直播 and 本場摘要 (both `session`), so each block names its own heading.
const headingId = useId()
// The state object never changes for a block, so its refs are unpacked once for the template.
const { shown, failed, notLive, checks, session, when, line, stats, exporting, exportNotice, exportIt } =
  props.stage
// 開播前檢查 starts unfolded.
const openChecks = ref(true)
</script>

<style scoped>
.stage {
  display: grid;
  gap: 0.5rem;
  padding: 1rem 1.125rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-900);
}

.stage h2 {
  margin: 0;
}

.muted,
.label {
  color: var(--p-text-muted-color);
}

.line {
  margin: 0;
  font-variant-numeric: tabular-nums;
}

.head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.25rem 1rem;
}

.ready {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

/* One row per check with a line between rows, as in the prototype. */
.plain {
  list-style: none;
  padding: 0;
}

.checks {
  display: grid;
  margin: 0;
}

.checks li {
  display: flex;
  align-items: center;
  gap: 0.625rem;
  min-height: 2.75rem;
  padding: 0.375rem 0;
}

.checks li + li {
  border-top: 1px solid var(--p-surface-700);
}

.checks .ok > .pi {
  color: var(--p-green-400);
}

.checks .warn > .pi {
  color: var(--p-yellow-400);
}

.checks .bad > .pi {
  color: var(--p-red-400);
}

.fix {
  margin-left: auto;
}

.export {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
}

.export p {
  margin: 0;
}

.stats {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1.5rem;
}

.stats li {
  display: grid;
}

.value {
  font-size: 1.25rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

button.value.masked.inline {
  font-size: inherit;
}

button.value.masked {
  padding: 0;
  border: 0;
  background: none;
  color: var(--p-text-muted-color);
  font-family: inherit;
  text-align: start;
  cursor: pointer;
}

.label {
  font-size: 0.8125rem;
}
</style>
