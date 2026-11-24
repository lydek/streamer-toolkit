<template>
  <!-- 計數器; 訂閱馬拉松 is its own page (MarathonPage.vue). -->
  <FullPage tile="counter" overlay="counter">
    <section class="stack" aria-labelledby="counters-heading">
      <h2 id="counters-heading">{{ t('counter.heading') }}</h2>
      <form class="create" @submit.prevent="create">
        <div class="field">
          <label for="new-counter-name">{{ t('counter.name') }}</label>
          <InputText id="new-counter-name" v-model="newName" autocomplete="off" maxlength="100" />
        </div>
        <div class="field">
          <label for="new-counter-command">{{ t('counter.command') }}</label>
          <span class="command">
            <span aria-hidden="true">!</span>
            <InputText id="new-counter-command" v-model="newCommand" autocomplete="off" />
          </span>
        </div>
        <Button type="submit" :label="t('counter.add')" icon="pi pi-plus" :disabled="!newName.trim() || busy" />
      </form>
      <p v-if="failed" role="alert">{{ failed }}</p>
      <p v-else-if="!counters">{{ t('app.loading') }}</p>
      <p v-else-if="!counters.length">{{ t('counter.empty') }}</p>
      <ul v-else class="rows">
        <CounterRow
          v-for="c in counters"
          :key="c.id"
          :counter="c"
          :value="valueOf(c)"
          @add="(d) => step(c, d)"
          @reset="reset(c)"
          @deleted="deleted"
          @error="report"
        />
      </ul>
    </section>

    <section class="stack" aria-labelledby="viewer-counters-heading">
      <h2 id="viewer-counters-heading">{{ t('counter.viewers.heading') }}</h2>
      <form class="create" @submit.prevent="createViewerCounter">
        <div class="field">
          <label for="new-viewer-counter-name">{{ t('counter.name') }}</label>
          <InputText id="new-viewer-counter-name" v-model="newViewerName" autocomplete="off" maxlength="100" />
        </div>
        <Button type="submit" :label="t('counter.viewers.add')" icon="pi pi-plus" :disabled="!newViewerName.trim() || busy" />
      </form>
      <p v-if="viewerCounters?.length === 0">{{ t('counter.empty') }}</p>
      <ul v-else-if="viewerCounters" class="rows">
        <ViewerCounterRow
          v-for="c in viewerCounters"
          :key="c.id"
          :counter="c"
          @changed="loadViewerCounters"
          @deleted="viewerCounterDeleted"
          @error="report"
        />
      </ul>
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { stepCounter, resetCounter } from './counters'
import CounterRow from './CounterRow.vue'
import FullPage from './FullPage.vue'
import { errorToast, successToast, useLiveEvents } from './home'
import { keepFocus } from './page'
import ViewerCounterRow from './ViewerCounterRow.vue'

const { t } = useI18n()
const toast = useToast()
const live = useLiveEvents()
const counters = shallowRef<Schemas['CounterBody'][]>()
const failed = ref('')
const newName = ref('')
const newCommand = ref('')
const busy = ref(false)
const viewerCounters = shallowRef<Schemas['ViewerCounterBody'][]>()
const newViewerName = ref('')

const report = (e: unknown) =>
  toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
// Chat commands change values without this page asking; the live values are the newest.
const valueOf = (c: Schemas['CounterBody']) => live.value.counters?.find((v) => v.id === c.id)?.value ?? c.value

async function load() {
  try {
    counters.value = await api(client.GET('/api/admin/counters'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : t('counter.loadFailed')
  }
}

async function create() {
  busy.value = true
  try {
    await api(
      client.POST('/api/admin/counters', {
        body: { name: newName.value, command: newCommand.value.trim().replace(/^!/, ''), allowed: 'moderator', per_stream: false },
      }),
    )
    newName.value = ''
    newCommand.value = ''
    await load()
  } catch (e) {
    report(e)
  } finally {
    busy.value = false
  }
}

async function loadViewerCounters() {
  try {
    viewerCounters.value = await api(client.GET('/api/admin/counters/viewers'))
  } catch (e) {
    report(e)
  }
}

async function createViewerCounter() {
  busy.value = true
  try {
    await api(client.POST('/api/admin/counters/viewers', { body: { name: newViewerName.value } }))
    newViewerName.value = ''
    await loadViewerCounters()
  } catch (e) {
    report(e)
  } finally {
    busy.value = false
  }
}

async function viewerCounterDeleted(c: Schemas['ViewerCounterBody']) {
  toast.add(successToast(t('counter.deleted', { name: c.name })))
  await loadViewerCounters()
  await keepFocus(() => document.getElementById('new-viewer-counter-name') ?? undefined)
}

const step = (c: Schemas['CounterBody'], delta: 1 | -1) => stepCounter(c.id, delta).catch(report)
const reset = (c: Schemas['CounterBody']) => resetCounter(c.id).catch(report)

async function deleted(c: Schemas['CounterBody']) {
  toast.add(successToast(t('counter.deleted', { name: c.name })))
  await load()
  // The deleted row took the focused button with it.
  await keepFocus(() => document.getElementById('new-counter-name') ?? undefined)
}


void load()
void loadViewerCounters()
</script>

<style scoped>
.copy {
  align-self: flex-start;
}

.rows {
  list-style: none;
  padding: 0;
  margin: 0;
}

.create {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.5rem 1rem;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.command {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
</style>
