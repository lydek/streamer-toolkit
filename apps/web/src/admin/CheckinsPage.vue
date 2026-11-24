<template>
  <FullPage tile="checkin" overlay="checkin">
    <template #actions>
      <Button :label="t('checkin.push.hide')" icon="pi pi-eye-slash" outlined @click="hideCard" />
      <Button :label="t('checkin.import.heading')" icon="pi pi-upload" outlined @click="openImport" />
    </template>
    <form class="filters" :aria-label="t('checkin.page.filters')" @submit.prevent="apply">
      <div class="field">
        <label for="cf-search">{{ t('checkin.page.search') }}</label>
        <InputText id="cf-search" v-model="search" autocomplete="off" />
      </div>
      <div class="buttons">
        <Button type="submit" :label="t('redemption.apply')" icon="pi pi-search" />
      </div>
      <div class="field">
        <label for="cf-date">{{ t('checkin.page.date') }}</label>
        <InputText id="cf-date" v-model="date" type="date" :max="today" />
      </div>
    </form>

    <!-- Always in the DOM, so screen readers announce changes to the count. -->
    <p role="status">{{ page ? t('checkin.page.total', { n: page.total }) : '' }}</p>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!page">{{ t('app.loading') }}</p>
    <p v-else-if="page.items.length === 0">{{ applied ? t('redemption.noMatch') : t('checkin.page.empty') }}</p>
    <ul v-else class="viewers">
      <li v-for="v in page.items" :key="v.id">
        <span class="name">
          {{ v.user_name }} <span class="login">{{ v.user_login }}</span>
        </span>
        <span>{{ t('checkin.page.totalOf', { n: v.total }) }}</span>
        <span>{{ t('checkin.deduct.available', { n: v.available }) }}</span>
        <span>{{ t('checkin.page.card', { stamps: v.stamps, slots: v.card_slots }) }}</span>
        <span>{{ v.last_check_in ? t('checkin.page.last', { date: v.last_check_in }) : t('checkin.page.never') }}</span>
        <Button
          :label="date === today ? t('checkin.page.registerToday') : t('checkin.page.registerDay', { date })"
          size="small"
          outlined
          :disabled="busy || !date"
          @click="register(v)"
        />
        <Button :label="t('checkin.push.show')" size="small" text @click="pushCard(v)" />
        <Button
          :label="t('checkin.deduct.open')"
          size="small"
          text
          :disabled="!v.available"
          :aria-label="t('checkin.deduct.for', { name: v.user_name })"
          @click="openDeduct(v)"
        />
      </li>
    </ul>

    <nav v-if="page && pages > 1" class="pager" :aria-label="t('redemption.pages')">
      <Button :label="t('redemption.previous')" icon="pi pi-angle-left" text :disabled="pageIndex === 0" @click="go(-1)" />
      <span>{{ t('redemption.page', { page: pageIndex + 1, pages }) }}</span>
      <Button
        :label="t('redemption.next')"
        icon="pi pi-angle-right"
        icon-pos="right"
        text
        :disabled="pageIndex + 1 >= pages"
        @click="go(1)"
      />
    </nav>

    <CheckinDeductions ref="log" />
    <!-- 扣印章: empty each time it opens; Esc or 取消 sends nothing. -->
    <dialog ref="deductEl" class="sheet deduct" aria-labelledby="deduct-title" @close="deducting = undefined">
      <form v-if="deducting" class="stack" @submit.prevent="deduct(deducting)">
        <h2 id="deduct-title">{{ t('checkin.deduct.for', { name: deducting.user_name }) }}</h2>
        <p>{{ t('checkin.deduct.available', { n: deducting.available }) }}</p>
        <label for="deduct-amount">{{ t('checkin.deduct.amount') }}</label>
        <InputNumber
          v-model="amount"
          input-id="deduct-amount"
          :min="1"
          :max="deducting.available"
          :allow-empty="false"
          :use-grouping="false"
        />
        <label for="deduct-reason">{{ t('checkin.deduct.reason') }}</label>
        <InputText id="deduct-reason" v-model="reason" maxlength="100" autocomplete="off" />
        <div class="buttons sheet-actions">
          <Button type="submit" :label="t('checkin.deduct.confirm')" :disabled="busy" />
          <Button :label="t('checkin.deduct.cancel')" text @click="deductEl?.close()" />
        </div>
      </form>
    </dialog>

    <!-- A fresh import each time it opens; Esc or 關閉 cancels it. -->
    <dialog ref="importEl" class="sheet import" aria-labelledby="checkin-import" @close="importing = false">
      <CheckinImport v-if="importing" @imported="imported" />
      <div class="sheet-actions">
        <Button :label="t('checkin.import.close')" text @click="importEl?.close()" />
      </div>
    </dialog>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import CheckinDeductions from './CheckinDeductions.vue'
import CheckinImport from './CheckinImport.vue'
import { changesCheckins, localDate, viewerQuery } from './checkins'
import FullPage from './FullPage.vue'
import { errorToast, successToast, useLiveEvents } from './home'
import { pageCount } from './redemptions'

const { t } = useI18n()
const toast = useToast()

const importEl = ref<HTMLDialogElement>()
const importing = ref(false)
function openImport() {
  importing.value = true
  importEl.value?.showModal()
}
function imported() {
  importEl.value?.close()
  void load()
}

const search = ref('')
/** The search of the list shown; the form applies on submit. */
const applied = ref('')
const today = ref(localDate())
/** Day the buttons register: today, or a past day to back-fill. */
const date = ref(today.value)
const pageIndex = ref(0)
const page = shallowRef<Schemas['CheckinViewerPage']>()
const failed = ref('')
const busy = ref(false)
let request = 0
// The viewer whose deduction form is open.
const deductEl = ref<HTMLDialogElement>()
const deducting = ref<Schemas['CheckinViewer']>()
const amount = ref(1)
const reason = ref('')
const log = ref<InstanceType<typeof CheckinDeductions>>()

const pages = computed(() => pageCount(page.value?.total ?? 0))

async function load() {
  const mine = ++request
  try {
    const result = await api(
      client.GET('/api/admin/checkin/viewers', { params: { query: viewerQuery(applied.value, pageIndex.value) } }),
    )
    // A slower earlier answer never replaces a newer one.
    if (mine !== request) return
    page.value = result
    failed.value = ''
  } catch (e) {
    if (mine !== request) return
    failed.value = e instanceof ApiError ? errorMessage(e.code) : t('checkin.page.loadFailed')
  }
}

function apply() {
  applied.value = search.value.trim()
  pageIndex.value = 0
  void load()
}

function go(step: -1 | 1) {
  pageIndex.value += step
  void load()
}

/** The page may stay open past midnight: a date left on today moves on with it. */
function refreshToday() {
  const now = localDate()
  if (now === today.value) return
  if (date.value === today.value) date.value = now
  today.value = now
}

async function register(v: Schemas['CheckinViewer']) {
  busy.value = true
  refreshToday()
  const day = date.value === today.value ? null : date.value
  try {
    const { result } = await api(
      client.POST('/api/admin/checkin/viewers/{id}/check-ins', {
        params: { path: { id: v.id } },
        body: { date: day },
      }),
    )
    const shown = day ?? today.value
    toast.add(
      result === 'checked_in'
        ? successToast(t('checkin.page.registered', { name: v.user_name, date: shown }))
        : { severity: 'info', summary: t('checkin.page.already', { name: v.user_name, date: shown }), life: 4000 },
    )
    await load()
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = false
  }
}

async function pushCard(v: Schemas['CheckinViewer']) {
  try {
    await api(client.POST('/api/admin/checkin/viewers/{id}/card', { params: { path: { id: v.id } } }))
    toast.add(successToast(t('checkin.push.queued', { name: v.user_name })))
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  }
}

async function hideCard() {
  try {
    await api(client.POST('/api/admin/checkin/card/hide'))
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  }
}

function openDeduct(v: Schemas['CheckinViewer']) {
  deducting.value = v
  amount.value = 1
  reason.value = ''
  deductEl.value?.showModal()
}

async function deduct(v: Schemas['CheckinViewer']) {
  busy.value = true
  try {
    const { available } = await api(
      client.POST('/api/admin/checkin/viewers/{id}/deductions', {
        params: { path: { id: v.id } },
        body: { amount: amount.value, reason: reason.value },
      }),
    )
    deductEl.value?.close()
    toast.add(successToast(t('checkin.deduct.done', { name: v.user_name, n: amount.value, left: available })))
    await Promise.all([load(), log.value?.load()])
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
    // Another deduction may have spent them meanwhile.
    await load()
  } finally {
    busy.value = false
  }
}

// Loaded right away, and again on every check-in and reconnect.
void load()
window.addEventListener('focus', refreshToday)
document.addEventListener('visibilitychange', refreshToday)
onBeforeUnmount(() => {
  window.removeEventListener('focus', refreshToday)
  document.removeEventListener('visibilitychange', refreshToday)
})
useLiveEvents(2000, (msg) => {
  if (changesCheckins(msg)) void load()
})
</script>

<style scoped>
.filters {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.75rem 1rem;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.field :deep(.p-inputtext) {
  min-width: 10rem;
  max-width: 100%;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.viewers {
  list-style: none;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.viewers li {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.25rem 1rem;
  padding: 0.5rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}

.name {
  font-weight: 600;
  overflow-wrap: anywhere;
}

.login {
  font-weight: 400;
  color: var(--p-text-muted-color);
}

.deduct :deep(.p-inputnumber-input) {
  width: 5rem;
}

.pager {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: 0.5rem;
  margin-block: 1rem;
}

.import {
  max-width: 36rem;
}

.deduct {
  max-width: 28rem;
}
</style>
