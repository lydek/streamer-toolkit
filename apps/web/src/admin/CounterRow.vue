<template>
  <li class="counter ctr-card">
    <div class="ctr-top">
      <div class="ctr-meta">
        <b class="ctr-name">{{ name.value.value }}</b>
        <span class="muted">
          <template v-if="command.value.value">!{{ command.value.value }} · </template>
          {{ t(perStream.value.value ? 'counter.perStream' : 'counter.keepAcrossStreams') }}
        </span>
      </div>
      <div class="ctr-tools">
        <Button
          icon="pi pi-pencil"
          text
          size="small"
          :aria-label="t('counter.editFor', { name: name.value.value })"
          :aria-expanded="editing"
          :aria-controls="`counter-edit-${counter.id}`"
          @click="toggleEdit"
        />
        <Button
          ref="moreButton"
          icon="pi pi-ellipsis-h"
          text
          size="small"
          :aria-label="t('counter.more', { name: name.value.value })"
          aria-haspopup="true"
          @click="menu?.toggle($event)"
        />
        <Menu ref="menu" :model="menuItems" popup />
      </div>
    </div>

    <output class="value ctr-num" :aria-label="name.value.value">{{ value }}</output>

    <div class="ctr-act">
      <Button
        icon="pi pi-minus"
        severity="secondary"
        :aria-label="t('counter.minusOneFor', { name: name.value.value })"
        @click="emit('add', -1)"
      />
      <Button :label="t('counter.plusOne')" icon="pi pi-plus" @click="emit('add', 1)" />
      <Button :label="t('counter.reset')" text @click="emit('reset')" />
    </div>

    <div v-show="editing" :id="`counter-edit-${counter.id}`" class="edit-box">
      <div class="field">
        <label :for="`counter-name-${counter.id}`">{{ t('counter.name') }}</label>
        <InputText
          :id="`counter-name-${counter.id}`"
          v-model="name.value.value"
          autocomplete="off"
          maxlength="100"
          @keydown.enter="name.commit()"
          @blur="name.commit()"
        />
      </div>
      <div class="field">
        <label :for="`counter-command-${counter.id}`">{{ t('counter.command') }}</label>
        <span class="command">
          <span aria-hidden="true">!</span>
          <InputText
            :id="`counter-command-${counter.id}`"
            v-model="command.value.value"
            autocomplete="off"
            @keydown.enter="commitCommand"
            @blur="commitCommand"
          />
        </span>
      </div>
      <div class="field">
        <label :id="`counter-allowed-${counter.id}`">{{ t('commands.allowed.label') }}</label>
        <Select
          v-model="allowed.value.value"
          :options="allowedOptions"
          option-label="label"
          option-value="value"
          :aria-labelledby="`counter-allowed-${counter.id}`"
          @update:model-value="allowed.commit()"
        />
      </div>
      <span class="choice">
        <Checkbox
          v-model="perStream.value.value"
          :input-id="`counter-per-stream-${counter.id}`"
          binary
          @update:model-value="perStream.commit()"
        />
        <label :for="`counter-per-stream-${counter.id}`">{{ t('counter.perStream') }}</label>
      </span>
    </div>

    <div v-if="confirming" class="confirm" role="group" :aria-label="t('counter.delete')">
      <p>{{ t('counter.deleteNote') }}</p>
      <div class="buttons">
        <Button ref="confirmButton" :label="t('counter.confirmDelete')" severity="danger" :disabled="busy" @click="remove" />
        <Button :label="t('counter.cancel')" text @click="cancel" />
      </div>
    </div>
  </li>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import Menu from 'primevue/menu'
import type { MenuItem } from 'primevue/menuitem'
import Select from 'primevue/select'
import { computed, nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'
import { settingQueue, useDrawerSetting } from './drawer'

const props = defineProps<{
  counter: Schemas['CounterBody']
  /** The live value, newer than the one loaded with the list. */
  value: number
}>()
const emit = defineEmits<{ add: [delta: 1 | -1]; reset: []; deleted: [Schemas['CounterBody']]; error: [unknown] }>()
const { t } = useI18n()

// PUT takes every setting, so each save sends the others as last saved.
let saved = { ...props.counter }
async function put(patch: Partial<Schemas['CounterInput']>) {
  const { name, command, allowed, per_stream } = { ...saved, ...patch }
  saved = await api(
    client.PUT('/api/admin/counters/{id}', {
      params: { path: { id: props.counter.id } },
      body: { name, command, allowed, per_stream },
    }),
  )
}
const queue = settingQueue()
const name = useDrawerSetting(ref(props.counter.name), (v) => put({ name: v }), queue)
const command = useDrawerSetting(ref(props.counter.command), (v) => put({ command: v }), queue)
// The "!" is shown in front of the field, so a typed one is dropped (the Host drops it too).
function commitCommand() {
  command.value.value = command.value.value.trim().replace(/^!/, '')
  void command.commit()
}
const allowed = useDrawerSetting(ref(props.counter.allowed), (v) => put({ allowed: v }), queue)
const perStream = useDrawerSetting(ref(props.counter.per_stream), (v) => put({ per_stream: v }), queue)
const allowedOptions = computed(() =>
  (['everyone', 'subscriber', 'vip', 'moderator'] as const).map((value) => ({ value, label: t(`commands.allowed.${value}`) })),
)

const editing = ref(false)
async function toggleEdit() {
  editing.value = !editing.value
  if (editing.value) {
    await nextTick()
    document.getElementById(`counter-name-${props.counter.id}`)?.focus()
  }
}
const menu = ref<InstanceType<typeof Menu>>()
const menuItems = computed<MenuItem[]>(() => [
  { label: t('counter.minusOne'), icon: 'pi pi-minus', command: () => emit('add', -1) },
  { label: t('counter.delete'), icon: 'pi pi-trash', command: () => void ask() },
])

const confirming = ref(false)
const busy = ref(false)
const moreButton = ref<{ $el: HTMLElement }>()
const confirmButton = ref<{ $el: HTMLElement }>()
async function ask() {
  confirming.value = true
  await nextTick()
  confirmButton.value?.$el.focus()
}
async function cancel() {
  confirming.value = false
  await nextTick()
  moreButton.value?.$el.focus()
}
async function remove() {
  busy.value = true
  try {
    await api(client.DELETE('/api/admin/counters/{id}', { params: { path: { id: props.counter.id } } }))
    emit('deleted', props.counter)
  } catch (e) {
    emit('error', e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.ctr-card {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 1rem 1.125rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.ctr-top {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.5rem;
}

.ctr-meta {
  display: flex;
  flex-direction: column;
  gap: 0.125rem;
  min-width: 0;
}

.ctr-name {
  font-size: 1rem;
  overflow-wrap: anywhere;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.ctr-tools {
  display: flex;
  align-items: center;
  gap: 0.125rem;
  flex-shrink: 0;
}

.ctr-num {
  font-size: 2.25rem;
  line-height: 1.1;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.ctr-act {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

.edit-box {
  display: flex;
  flex-direction: column;
  gap: 0.625rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--p-surface-800);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.field :deep(.p-inputtext),
.field :deep(.p-select) {
  width: 100%;
}

.command,
.choice,
.buttons {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.confirm {
  padding-top: 0.5rem;
  border-top: 1px solid var(--p-surface-800);
}

.confirm p {
  margin: 0 0 0.5rem;
}
</style>
