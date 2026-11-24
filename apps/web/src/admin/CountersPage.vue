<template>
  <!-- 計數器; 訂閱馬拉松 is its own page (MarathonPage.vue). -->
  <FullPage tile="counter" overlay="counter">
    <template #actions>
      <Button
        ref="addCounterBtn"
        :label="t('counter.add')"
        icon="pi pi-plus"
        :aria-expanded="addingCounter"
        @click="toggleAddCounter"
      />
    </template>

    <section class="section" aria-labelledby="counters-heading">
      <div class="sec-head">
        <h2 id="counters-heading">{{ t('counter.heading') }}</h2>
      </div>
      <form v-show="addingCounter" class="add-panel" @submit.prevent="create">
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
        <div class="form-actions">
          <Button type="submit" :label="t('counter.add')" icon="pi pi-plus" :disabled="!newName.trim() || busy" />
          <Button :label="t('counter.cancel')" text @click="closeAddCounter" />
        </div>
      </form>
      <p v-if="failed" role="alert">{{ failed }}</p>
      <p v-else-if="!counters">{{ t('app.loading') }}</p>
      <p v-else-if="!counters.length">{{ t('counter.empty') }}</p>
      <ul v-else class="ctr-grid">
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

    <section class="section viewer-section" aria-labelledby="viewer-counters-heading">
      <div class="sec-head">
        <h2 id="viewer-counters-heading">{{ t('counter.viewers.heading') }}</h2>
        <span class="muted">{{ t('counter.viewers.hint') }}</span>
      </div>
      <div class="tbl-wrap viewer-wrap">
        <table v-if="viewerCounters?.length" class="table">
          <thead>
            <tr>
              <th scope="col">{{ t('counter.viewers.column') }}</th>
              <th scope="col" class="num">{{ t('counter.viewers.totalCol') }}</th>
              <th scope="col">{{ t('counter.viewers.top') }}</th>
              <th scope="col"><span class="sr-only">{{ t('screenTools.media.actions') }}</span></th>
            </tr>
          </thead>
          <ViewerCounterRow
            v-for="c in viewerCounters"
            :key="c.id"
            :counter="c"
            @changed="loadViewerCounters"
            @deleted="viewerCounterDeleted"
            @error="report"
          />
        </table>
        <p v-else-if="viewerCounters?.length === 0" class="empty-cell">{{ t('counter.empty') }}</p>
        <div class="tbl-foot">
          <span class="muted"></span>
          <Button
            ref="addViewerBtn"
            :label="t('counter.viewers.addShort')"
            icon="pi pi-plus"
            text
            :aria-expanded="addingViewer"
            @click="toggleAddViewer"
          />
        </div>
      </div>
      <form v-show="addingViewer" class="add-panel" @submit.prevent="createViewerCounter">
        <div class="field">
          <label for="new-viewer-counter-name">{{ t('counter.name') }}</label>
          <InputText id="new-viewer-counter-name" v-model="newViewerName" autocomplete="off" maxlength="100" />
        </div>
        <div class="form-actions">
          <Button type="submit" :label="t('counter.viewers.add')" icon="pi pi-plus" :disabled="!newViewerName.trim() || busy" />
          <Button :label="t('counter.cancel')" text @click="closeAddViewer" />
        </div>
      </form>
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { nextTick, ref, shallowRef } from 'vue'
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
const addingCounter = ref(false)
const addCounterBtn = ref<{ $el: HTMLElement }>()

const viewerCounters = shallowRef<Schemas['ViewerCounterBody'][]>()
const newViewerName = ref('')
const addingViewer = ref(false)
const addViewerBtn = ref<{ $el: HTMLElement }>()

const report = (e: unknown) =>
  toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
// Chat commands change values without this page asking; the live values are the newest.
const valueOf = (c: Schemas['CounterBody']) => live.value.counters?.find((v) => v.id === c.id)?.value ?? c.value

async function toggleAddCounter() {
  addingCounter.value = !addingCounter.value
  if (addingCounter.value) {
    await nextTick()
    document.getElementById('new-counter-name')?.focus()
  }
}

async function closeAddCounter() {
  addingCounter.value = false
  await nextTick()
  addCounterBtn.value?.$el.focus()
}

async function toggleAddViewer() {
  addingViewer.value = !addingViewer.value
  if (addingViewer.value) {
    await nextTick()
    document.getElementById('new-viewer-counter-name')?.focus()
  }
}

async function closeAddViewer() {
  addingViewer.value = false
  await nextTick()
  addViewerBtn.value?.$el.focus()
}

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
    void closeAddCounter()
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
    void closeAddViewer()
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
  await keepFocus(() => addViewerBtn.value?.$el)
}

const step = (c: Schemas['CounterBody'], delta: 1 | -1) => stepCounter(c.id, delta).catch(report)
const reset = (c: Schemas['CounterBody']) => resetCounter(c.id).catch(report)

async function deleted(c: Schemas['CounterBody']) {
  toast.add(successToast(t('counter.deleted', { name: c.name })))
  await load()
  // The deleted card took the focused button with it.
  await keepFocus(() => addCounterBtn.value?.$el)
}

void load()
void loadViewerCounters()
</script>

<style scoped>
.section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.viewer-section {
  margin-top: 1.5rem;
}

.sec-head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 0.5rem 0.75rem;
}

.sec-head h2 {
  margin: 0;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.ctr-grid {
  list-style: none;
  padding: 0;
  margin: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(17rem, 1fr));
  gap: 0.875rem;
}

.viewer-wrap {
  max-width: 52rem;
  margin-top: 0;
}

.empty-cell {
  margin: 0;
  padding: 1rem 0.75rem;
  color: var(--p-text-muted-color);
}

.add-panel {
  flex-direction: row;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.75rem 1rem;
  max-width: 52rem;
  padding: 0.875rem 1rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.command,
.form-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}
</style>
