<template>
  <li class="sound">
    <div class="field">
      <label :for="`sound-label-${sound.id}`">{{ t('sound.label') }}</label>
      <InputText
        :id="`sound-label-${sound.id}`"
        v-model="label.value.value"
        autocomplete="off"
        maxlength="100"
        @keydown.enter="label.commit()"
        @blur="label.commit()"
      />
    </div>
    <div class="field">
      <label :for="`sound-volume-${sound.id}`">{{ t('sound.volume') }}</label>
      <InputNumber
        v-model="volume.value.value"
        :input-id="`sound-volume-${sound.id}`"
        :min="0"
        :max="100"
        suffix="%"
        :allow-empty="false"
        :use-grouping="false"
        @keydown.enter="volume.commit()"
        @blur="volume.commit()"
      />
    </div>
    <div class="buttons">
      <Button :label="t('sound.play')" icon="pi pi-play" outlined @click="play" />
      <Button ref="deleteButton" :label="t('sound.delete')" icon="pi pi-trash" severity="danger" text @click="ask" />
    </div>
    <div v-if="confirming" class="confirm" role="group" :aria-label="t('sound.delete')">
      <template v-if="usedBy.length">
        <p>{{ t('sound.usedBy') }}</p>
        <ul>
          <li v-for="u in usedBy" :key="u">{{ u }}</li>
        </ul>
      </template>
      <p v-else>{{ t('sound.notUsed') }}</p>
      <div class="buttons">
        <Button ref="confirmButton" :label="t('sound.confirmDelete')" severity="danger" :disabled="busy" @click="remove" />
        <Button :label="t('sound.cancel')" text @click="cancel" />
      </div>
    </div>
  </li>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import { nextTick, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'
import { useDrawerSetting } from './drawer'

const props = defineProps<{ sound: Schemas['Sound']; usedBy: string[] }>()
const emit = defineEmits<{ deleted: [Schemas['Sound']]; error: [unknown] }>()
const { t } = useI18n()

// PUT takes both values, so saves run one at a time from the last saved sound and never undo each other.
let saved = { label: props.sound.label, volume: props.sound.volume }
let queue: Promise<unknown> = Promise.resolve()
function put(patch: Partial<typeof saved>) {
  const run = queue.then(async () => {
    const s = await api(
      client.PUT('/api/admin/sounds/{id}', { params: { path: { id: props.sound.id } }, body: { ...saved, ...patch } }),
    )
    saved = { label: s.label, volume: s.volume }
  })
  queue = run.catch(() => {})
  return run
}
const labelSaved = ref(props.sound.label)
const label = useDrawerSetting(labelSaved, (v) => put({ label: v }))
const volumeSaved = ref(props.sound.volume)
const volume = useDrawerSetting(volumeSaved, (v) => put({ volume: v }))

let audio: HTMLAudioElement | undefined
function play() {
  audio?.pause()
  // The read-only sound route; the admin window is on the Host's origin.
  audio = new Audio(`/sounds/${props.sound.id}`)
  audio.volume = volume.value.value / 100
  audio.play()?.catch(() => {})
}
onBeforeUnmount(() => audio?.pause())

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
    await api(client.DELETE('/api/admin/sounds/{id}', { params: { path: { id: props.sound.id } } }))
    emit('deleted', props.sound)
  } catch (e) {
    emit('error', e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.sound {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.5rem 1rem;
  padding: 0.5rem 0;
  border-bottom: 1px solid var(--p-content-border-color);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.field :deep(.p-inputtext) {
  max-width: 100%;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.confirm {
  flex-basis: 100%;
  overflow-wrap: anywhere;
}

.confirm p,
.confirm ul {
  margin: 0 0 0.5rem;
}
</style>
