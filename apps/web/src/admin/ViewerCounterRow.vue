<template>
  <li class="viewer-counter">
    <div class="head">
      <div class="field">
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
      <span class="total">{{ t('counter.viewers.total', { n: counter.total }) }}</span>
      <Button ref="deleteButton" :label="t('counter.delete')" icon="pi pi-trash" severity="danger" text @click="ask" />
    </div>
    <p class="top">
      <span class="label">{{ t('counter.viewers.top') }}</span>
      {{ counter.top.length ? counter.top.map((v) => `${v.display_name} ${v.value}`).join(' · ') : t('counter.viewers.none') }}
    </p>
    <div v-if="confirming" class="confirm" role="group" :aria-label="t('counter.delete')">
      <p>{{ t('counter.viewers.deleteNote') }}</p>
      <div class="buttons">
        <Button ref="confirmButton" :label="t('counter.confirmDelete')" severity="danger" :disabled="busy" @click="remove" />
        <Button :label="t('counter.cancel')" text @click="cancel" />
      </div>
    </div>

    <label :for="`viewer-search-${counter.id}`">{{ t('counter.viewers.search') }}</label>
    <InputText :id="`viewer-search-${counter.id}`" v-model="query" type="search" autocomplete="off" @input="searchSoon" />
    <p v-if="query.trim() && found?.length === 0">{{ t('counter.viewers.noMatch') }}</p>
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

    <Button
      :label="t(all ? 'counter.viewers.hideAll' : 'counter.viewers.showAll')"
      text
      :aria-expanded="!!all"
      @click="toggleAll"
    />
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
  </li>
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

const query = ref('')
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
.viewer-counter {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 0.75rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}

.head {
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

.total,
.value {
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.top,
.confirm p {
  margin: 0;
}

.label {
  color: var(--p-text-muted-color);
  margin-inline-end: 0.5rem;
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
}
</style>
