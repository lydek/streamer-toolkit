<template>
  <tbody class="viewer-counter">
    <tr>
      <th scope="row">
        <b>{{ name.value.value }}</b>
      </th>
      <td class="num">
        <span>{{ counter.total }}</span>
        <span class="sr-only">{{ t('counter.viewers.total', { n: counter.total }) }}</span>
      </td>
      <td class="top">
        {{ counter.top.length ? counter.top.map((v) => `${v.display_name} ${v.value}`).join(' · ') : t('counter.viewers.none') }}
      </td>
      <td class="end">
        <span class="row-actions">
          <Button
            ref="searchButton"
            icon="pi pi-search"
            text
            size="small"
            :aria-label="`${t('counter.viewers.search')}：${name.value.value}`"
            :aria-expanded="searching"
            :aria-controls="`viewer-detail-${counter.id}`"
            @click="toggleSearch"
          />
          <Button
            :label="t(all ? 'counter.viewers.hideAll' : 'counter.viewers.showAll')"
            text
            :aria-expanded="!!all"
            :aria-controls="`viewer-detail-${counter.id}`"
            @click="toggleAll"
          />
          <Button
            icon="pi pi-pencil"
            text
            size="small"
            :aria-label="t('counter.editFor', { name: name.value.value })"
            :aria-expanded="editing"
            :aria-controls="`viewer-detail-${counter.id}`"
            @click="toggleEdit"
          />
          <Button
            ref="deleteButton"
            icon="pi pi-trash"
            severity="danger"
            text
            size="small"
            :aria-label="`${t('counter.delete')}：${name.value.value}`"
            @click="ask"
          />
        </span>
      </td>
    </tr>

    <tr v-show="editing || searching || all || confirming" :id="`viewer-detail-${counter.id}`" class="detail-row">
      <td colspan="4">
        <div class="detail">
          <div v-if="editing" class="field">
            <label :for="`viewer-counter-name-${counter.id}`">{{ t('counter.name') }}</label>
            <InputText
              :id="`viewer-counter-name-${counter.id}`"
              v-model="name.value.value"
              autocomplete="off"
              maxlength="100"
              @keydown.enter="name.commit()"
              @blur="name.commit()"
            />
          </div>

          <div v-if="confirming" class="confirm" role="group" :aria-label="t('counter.delete')">
            <p>{{ t('counter.viewers.deleteNote') }}</p>
            <div class="buttons">
              <Button ref="confirmButton" :label="t('counter.confirmDelete')" severity="danger" :disabled="busy" @click="remove" />
              <Button :label="t('counter.cancel')" text @click="cancel" />
            </div>
          </div>

          <div class="field">
            <label :for="`viewer-search-${counter.id}`">{{ t('counter.viewers.search') }}</label>
            <InputText :id="`viewer-search-${counter.id}`" v-model="query" type="search" autocomplete="off" @input="searchSoon" />
          </div>
          <p v-if="query.trim() && found?.length === 0" class="muted">{{ t('counter.viewers.noMatch') }}</p>
          <ul v-if="found?.length" class="viewers">
            <li v-for="v in found" :key="v.twitch_user_id">
              <span class="who">{{ t('counter.viewers.entry', { name: v.display_name, login: v.login }) }}</span>
              <output class="value">{{ v.value }}</output>
              <Button :label="t('counter.plusOne')" icon="pi pi-plus" size="small" @click="step(v, 1)" />
              <Button
                icon="pi pi-ellipsis-h"
                text
                size="small"
                :aria-label="t('counter.more', { name: v.display_name })"
                aria-haspopup="true"
                @click="openMenu(v, $event)"
              />
            </li>
          </ul>

          <ol v-if="all" class="viewers">
            <li v-if="!all.length">{{ t('counter.viewers.none') }}</li>
            <li v-for="v in all" :key="v.twitch_user_id">
              <span class="who">{{ t('counter.viewers.entry', { name: v.display_name, login: v.login }) }}</span>
              <output class="value">{{ v.value }}</output>
              <Button :label="t('counter.plusOne')" icon="pi pi-plus" size="small" @click="step(v, 1)" />
              <Button
                icon="pi pi-ellipsis-h"
                text
                size="small"
                :aria-label="t('counter.more', { name: v.display_name })"
                aria-haspopup="true"
                @click="openMenu(v, $event)"
              />
            </li>
          </ol>
          <Menu ref="menu" :model="menuItems" popup />
        </div>
      </td>
    </tr>
  </tbody>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Menu from 'primevue/menu'
import type { MenuItem } from 'primevue/menuitem'
import { computed, nextTick, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'
import { useDrawerSetting } from './drawer'

type Count = Schemas['ViewerCountBody']
const props = defineProps<{ counter: Schemas['ViewerCounterBody'] }>()
const emit = defineEmits<{ changed: []; deleted: [Schemas['ViewerCounterBody']]; error: [unknown] }>()
const { t } = useI18n()
const path = { params: { path: { id: props.counter.id } } }

const name = useDrawerSetting(ref(props.counter.name), async (v) => {
  await api(client.PUT('/api/admin/counters/viewers/{id}', { ...path, body: { name: v } }))
  emit('changed')
})

const editing = ref(false)
const searching = ref(false)
const searchButton = ref<{ $el: HTMLElement }>()
const query = ref('')

async function toggleEdit() {
  editing.value = !editing.value
  if (editing.value) {
    await nextTick()
    document.getElementById(`viewer-counter-name-${props.counter.id}`)?.focus()
  }
}

async function toggleSearch() {
  searching.value = !searching.value
  if (searching.value) {
    await nextTick()
    document.getElementById(`viewer-search-${props.counter.id}`)?.focus()
  } else {
    query.value = ''
    found.value = undefined
    await nextTick()
    searchButton.value?.$el.focus()
  }
}
const found = shallowRef<Count[]>()
let searchTimer: ReturnType<typeof setTimeout> | undefined
onBeforeUnmount(() => clearTimeout(searchTimer))
/** Searches once typing pauses, so each key press does not ask the Host. */
function searchSoon() {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(() => void search(), 250)
}
async function search() {
  const q = query.value.trim()
  if (!q) {
    found.value = undefined
    return
  }
  try {
    const result = await api(client.GET('/api/admin/counters/viewers/{id}/search', { params: { ...path.params, query: { q } } }))
    // A slower answer to an older query never replaces a newer one.
    if (q === query.value.trim()) found.value = result
  } catch (e) {
    emit('error', e)
  }
}

// "See everyone": loaded when opened, `undefined` while closed.
const all = shallowRef<Count[]>()
async function loadAll() {
  all.value = await api(client.GET('/api/admin/counters/viewers/{id}/counts', path))
}
async function toggleAll() {
  if (all.value) {
    all.value = undefined
    return
  }
  try {
    await loadAll()
  } catch (e) {
    emit('error', e)
  }
}

/** Adds or takes one, then shows the new counts everywhere on the page. */
async function step(v: Count, delta: 1 | -1) {
  try {
    await api(client.POST('/api/admin/counters/viewers/{id}/add', { ...path, body: { twitch_user_id: v.twitch_user_id, delta } }))
    await Promise.all([found.value && search(), all.value && loadAll()])
    emit('changed')
  } catch (e) {
    emit('error', e)
  }
}

const menu = ref<InstanceType<typeof Menu>>()
const target = shallowRef<Count>()
function openMenu(v: Count, event: Event) {
  target.value = v
  menu.value?.toggle(event)
}
const menuItems = computed<MenuItem[]>(() => [
  { label: t('counter.minusOne'), icon: 'pi pi-minus', command: () => target.value && void step(target.value, -1) },
])

const confirming = ref(false)
const busy = ref(false)
const deleteButton = ref<{ $el: HTMLElement }>()
const confirmButton = ref<{ $el: HTMLElement }>()
async function ask() {
  confirming.value = true
  await nextTick()
  confirmButton.value?.$el.focus()
}
async function cancel() {
  confirming.value = false
  await nextTick()
  deleteButton.value?.$el.focus()
}
async function remove() {
  busy.value = true
  try {
    await api(client.DELETE('/api/admin/counters/viewers/{id}', path))
    emit('deleted', props.counter)
  } catch (e) {
    emit('error', e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.num {
  text-align: right;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.end {
  text-align: right;
  white-space: nowrap;
}

.row-actions {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
}

.detail {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 0.5rem 0;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  max-width: 20rem;
}

.muted,
.confirm p {
  margin: 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.value {
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.viewers {
  list-style: none;
  margin: 0;
  padding: 0;
}

.viewers li {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  padding: 0.25rem 0;
}

.who {
  flex: 1 1 10rem;
  min-width: 0;
  overflow-wrap: anywhere;
}

.buttons {
  display: flex;
  gap: 0.5rem;
  margin-top: 0.5rem;
}
</style>
