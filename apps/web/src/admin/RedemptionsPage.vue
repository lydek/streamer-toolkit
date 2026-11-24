<template>
  <FullPage tile="redemption">
    <!-- A narrow page folds the filters behind this button (rules below). -->
    <Button
      class="filters-toggle"
      :label="filtered ? t('redemption.filtersActive') : t('redemption.filtersToggle')"
      icon="pi pi-filter"
      severity="secondary"
      outlined
      :aria-expanded="filtersOpen"
      aria-controls="redemption-filters"
      @click="filtersOpen = !filtersOpen"
    />
    <form
      id="redemption-filters"
      class="filters"
      :class="{ open: filtersOpen }"
      :aria-label="t('redemption.filters')"
      @submit.prevent="apply"
    >
      <div class="field">
        <label for="rf-from">{{ t('redemption.from') }}</label>
        <InputText id="rf-from" v-model="form.from" type="date" />
      </div>
      <div class="field">
        <label for="rf-to">{{ t('redemption.to') }}</label>
        <InputText id="rf-to" v-model="form.to" type="date" />
      </div>
      <div class="field grow">
        <label for="rf-viewer">{{ t('redemption.viewer') }}</label>
        <InputText id="rf-viewer" v-model="form.viewer" autocomplete="off" />
      </div>
      <div class="field grow">
        <label for="rf-reward">{{ t('redemption.reward') }}</label>
        <InputText id="rf-reward" v-model="form.reward" autocomplete="off" />
      </div>
      <div class="field">
        <label id="rf-status-label">{{ t('redemption.statusLabel') }}</label>
        <Select
          v-model="form.status"
          input-id="rf-status"
          :options="statusOptions"
          option-label="label"
          option-value="value"
          aria-labelledby="rf-status-label"
        />
      </div>
      <div class="buttons">
        <Button type="submit" :label="t('redemption.apply')" icon="pi pi-filter" />
        <Button type="button" :label="t('redemption.clear')" severity="secondary" text @click="clear" />
      </div>
    </form>

    <!-- Always in the DOM, so screen readers announce changes to the count. -->
    <p role="status">{{ page ? t('redemption.total', { n: page.total }) : '' }}</p>
    <p v-if="page?.failed_writes" role="alert">{{ t('redemption.writeFailed', { n: page.failed_writes }) }}</p>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!page">{{ t('app.loading') }}</p>
    <RedemptionList
      v-else
      :records="page.items"
      :empty="filtered ? t('redemption.noMatch') : t('redemption.empty')"
    />

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
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { computed, reactive, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { useLiveEvents } from './home'
import RedemptionList from './RedemptionList.vue'
import { type Filters, STATUSES, changesRecords, noFilters, pageCount, redemptionQuery } from './redemptions'

const { t } = useI18n()

const form = reactive<Filters>(noFilters())
/** The filters of the list shown; the form applies on submit. */
const applied = ref<Filters>(noFilters())
const pageIndex = ref(0)
const page = shallowRef<Schemas['RedemptionPage']>()
const failed = ref('')
const filtersOpen = ref(false)
let request = 0

const statusOptions = computed(() => [
  { label: t('redemption.allStatuses'), value: null },
  ...STATUSES.map((s) => ({ label: t(`redemption.status.${s}`), value: s })),
])
const pages = computed(() => pageCount(page.value?.total ?? 0))
const filtered = computed(() => JSON.stringify(applied.value) !== JSON.stringify(noFilters()))

async function load() {
  const mine = ++request
  try {
    const result = await api(
      client.GET('/api/admin/redemptions', { params: { query: redemptionQuery(applied.value, pageIndex.value) } }),
    )
    // A slower earlier answer never replaces a newer one.
    if (mine !== request) return
    page.value = result
    failed.value = ''
  } catch (e) {
    if (mine !== request) return
    failed.value = e instanceof ApiError ? errorMessage(e.code) : t('redemption.loadFailed')
  }
}

function apply() {
  applied.value = { ...form }
  pageIndex.value = 0
  void load()
}

function clear() {
  Object.assign(form, noFilters())
  apply()
}

function go(step: -1 | 1) {
  pageIndex.value += step
  void load()
}

// Loaded right away, and again on every change and reconnect.
void load()
useLiveEvents(2000, (msg) => {
  if (changesRecords(msg)) void load()
})
</script>

<style scoped>
.field :deep(.p-inputtext),
.field :deep(.p-select) {
  min-width: 9rem;
  max-width: 100%;
}

.filters-toggle {
  display: none;
}

/* A narrow page: five fields would fill the screen, so they fold behind the toggle and stack full width. */
@container view (max-width: 45rem) {
  .filters-toggle {
    display: inline-flex;
    margin-top: 0.75rem;
  }

  .filters:not(.open) {
    display: none;
  }

  .filters {
    flex-direction: column;
    align-items: stretch;
  }

  .grow {
    flex: none;
    max-width: none;
  }

  .field :deep(.p-inputtext),
  .field :deep(.p-select) {
    width: 100%;
  }
}

.pager {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: 0.5rem;
  margin-block: 1rem;
}
</style>
