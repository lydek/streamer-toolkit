<template>
  <p v-if="!library">{{ t(failed ? 'sound.loadFailed' : 'app.loading') }}</p>
  <div v-else class="stack">
    <template v-for="(s, i) in slots" :key="s.slot">
      <label :id="`sound-slot-${s.slot}`">{{ s.label }}</label>
      <div class="row">
        <Select
          v-model="fields[i].value.value"
          :input-id="`sound-slot-${s.slot}-input`"
          :options="options"
          option-label="label"
          option-value="value"
          :aria-labelledby="`sound-slot-${s.slot}`"
          @update:model-value="fields[i].commit()"
        />
        <Button
          v-if="playable"
          :label="t(testing === s.slot ? 'sound.stopInObs' : 'sound.playInObs')"
          :icon="testing === s.slot ? 'pi pi-stop' : 'pi pi-play'"
          outlined
          :aria-describedby="`sound-slot-${s.slot}`"
          :disabled="testing !== s.slot && !isFile(fields[i].value.value)"
          @click="testing === s.slot ? stop() : play(s.slot, fields[i].value.value)"
        />
        <!-- More controls for this slot, e.g. a test of what uses the sound. -->
        <slot name="actions" :slot-id="s.slot" :label="s.label" />
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Select from 'primevue/select'
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'
import { useDrawerSetting } from './drawer'

// Each slot is applied at once. DEFAULT is its default (the built-in sound for alerts, silence otherwise); not '',
// which Select shows as nothing chosen.
const DEFAULT = 'default'
// `playable`: each slot gets a button that plays its sound on the sound overlay, to test it in OBS.
const props = defineProps<{ slots: { slot: string; label: string }[]; builtIn?: boolean; playable?: boolean }>()
const emit = defineEmits<{ played: []; error: [unknown] }>()
const { t } = useI18n()
const library = shallowRef<Schemas['SoundLibrary']>()
const failed = ref(false)

const fields = props.slots.map(({ slot }) => {
  const saved = ref(DEFAULT)
  const field = useDrawerSetting(saved, (v) =>
    api(client.PUT('/api/admin/sounds/slots/{slot}', { params: { path: { slot } }, body: { sound: v === DEFAULT ? null : v } })),
  )
  return Object.assign(field, { saved })
})

const options = computed(() => [
  ...(props.builtIn
    ? [
        { value: DEFAULT, label: t('sound.builtIn') },
        { value: 'silent', label: t('sound.silent') },
      ]
    : [{ value: DEFAULT, label: t('sound.silent') }]),
  ...(library.value?.sounds ?? []).map((s) => ({ value: s.id, label: s.label })),
])

const isFile = (v: string) => v !== DEFAULT && v !== 'silent'

// The slot whose sound was last sent to OBS: its button stops it until the sound's length has passed (or, not yet
// measured, until pressed).
const testing = ref<string>()
let ended: ReturnType<typeof setTimeout> | undefined
function testOver() {
  clearTimeout(ended)
  testing.value = undefined
}
onBeforeUnmount(() => clearTimeout(ended))

async function play(slot: string, id: string) {
  try {
    await api(client.POST('/api/admin/sounds/{id}/play', { params: { path: { id } } }))
    testOver()
    testing.value = slot
    const ms = library.value?.sounds.find((s) => s.id === id)?.duration_ms
    if (ms) ended = setTimeout(testOver, ms)
    emit('played')
  } catch (e) {
    emit('error', e)
  }
}

async function stop() {
  try {
    await api(client.POST('/api/admin/sounds/stop'))
    testOver()
  } catch (e) {
    emit('error', e)
  }
}

onMounted(async () => {
  try {
    library.value = await api(client.GET('/api/admin/sounds'))
  } catch {
    failed.value = true
    return
  }
  props.slots.forEach(({ slot }, i) => {
    const chosen = library.value!.slots[slot] ?? DEFAULT
    // Without a built-in sound, "silent" is the default.
    fields[i].saved.value = !props.builtIn && chosen === 'silent' ? DEFAULT : chosen
  })
})
</script>

<style scoped>
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.row :deep(.p-select) {
  min-width: 0;
  max-width: 100%;
}
</style>
