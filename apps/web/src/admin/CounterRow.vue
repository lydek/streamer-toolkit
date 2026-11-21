<template>
  <li class="counter">
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
    <output class="value" :aria-label="counter.name">{{ value }}</output>
    <span class="buttons">
      <Button :label="t('counter.plusOne')" icon="pi pi-plus" @click="emit('add', 1)" />
      <Button :label="t('counter.reset')" severity="secondary" outlined @click="emit('reset')" />
      <Button
        ref="moreButton"
        icon="pi pi-ellipsis-h"
        text
        :aria-label="t('counter.more', { name: counter.name })"
        aria-haspopup="true"
        @click="menu?.toggle($event)"
      />
      <Menu ref="menu" :model="menuItems" popup />
    </span>
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
import { useDrawerSetting } from './drawer'

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
const name = useDrawerSetting(ref(props.counter.name), (v) => put({ name: v }))
const command = useDrawerSetting(ref(props.counter.command), (v) => put({ command: v }))
// The "!" is shown in front of the field, so a typed one is dropped (the Host drops it too).
function commitCommand() {
  command.value.value = command.value.value.trim().replace(/^!/, '')
  void command.commit()
}
const allowed = useDrawerSetting(ref(props.counter.allowed), (v) => put({ allowed: v }))
const perStream = useDrawerSetting(ref(props.counter.per_stream), (v) => put({ per_stream: v }))
const allowedOptions = computed(() =>
  (['everyone', 'subscriber', 'vip', 'moderator'] as const).map((value) => ({ value, label: t(`commands.allowed.${value}`) })),
)

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
.counter {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.75rem 1rem;
  padding: 0.75rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.command,
.choice,
.buttons {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.value {
  min-width: 3ch;
  font-size: 1.75rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.confirm {
  flex-basis: 100%;
}

.confirm p {
  margin: 0 0 0.5rem;
}
</style>
