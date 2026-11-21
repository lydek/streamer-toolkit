<template>
  <div class="stack">
    <p v-if="!obsScenes.length">{{ t('scene.notConnected') }}</p>
    <ul v-if="draft.length" class="rules">
      <li v-for="(rule, i) in draft" :key="rule.id || i" class="rule">
        <div class="field">
          <label :for="`rule-name-${i}`">{{ t('music.rules.name') }}</label>
          <InputText :id="`rule-name-${i}`" v-model="rule.name" autocomplete="off" maxlength="100" @change="save" />
        </div>
        <div class="field wide">
          <label :for="`rule-scenes-${i}`">{{ t('music.rules.scenes') }}</label>
          <MultiSelect
            v-model="rule.scenes"
            :input-id="`rule-scenes-${i}`"
            :options="sceneOptions"
            display="chip"
            @update:model-value="save"
          />
        </div>
        <div class="field">
          <label :for="`rule-playlist-${i}`">{{ t('music.rules.playlist') }}</label>
          <Select
            v-model="rule.playlist"
            :input-id="`rule-playlist-${i}`"
            :options="playlists"
            option-label="name"
            option-value="id"
            :placeholder="t('music.player.choosePlaylist')"
            @update:model-value="save"
          />
          <span v-if="!rule.playlist" class="gone">{{ t('music.rules.playlistGone') }}</span>
        </div>
        <div class="field">
          <span :id="`rule-order-${i}`">{{ t('music.rules.order') }}</span>
          <SelectButton
            v-model="rule.shuffle"
            :options="orders"
            option-label="label"
            option-value="value"
            :allow-empty="false"
            :aria-labelledby="`rule-order-${i}`"
            @update:model-value="save"
          />
        </div>
        <div class="field">
          <label :for="`rule-volume-${i}`">{{ t('music.rules.volume') }}</label>
          <span class="slider">
            <input :id="`rule-volume-${i}`" v-model.number="rule.volume" type="range" min="0" max="100" @change="save" />
            <output :for="`rule-volume-${i}`">{{ rule.volume }}%</output>
          </span>
        </div>
        <div class="choices">
          <span class="choice">
            <Checkbox v-model="rule.autoplay" :input-id="`rule-autoplay-${i}`" binary @update:model-value="save" />
            <label :for="`rule-autoplay-${i}`">{{ t('music.rules.autoplay') }}</label>
          </span>
          <span class="choice">
            <Checkbox v-model="rule.stop_on_leave" :input-id="`rule-leave-${i}`" binary @update:model-value="save" />
            <label :for="`rule-leave-${i}`">{{ t('music.rules.stopOnLeave') }}</label>
          </span>
          <span class="choice">
            <Checkbox
              v-model="rule.enabled"
              :input-id="`rule-enabled-${i}`"
              binary
              :disabled="!rule.playlist"
              @update:model-value="save"
            />
            <label :for="`rule-enabled-${i}`">{{ t('music.rules.enabled') }}</label>
          </span>
        </div>
        <Button
          :label="t('music.rules.delete')"
          icon="pi pi-trash"
          severity="danger"
          text
          @click="remove(i)"
        />
      </li>
    </ul>
    <ul v-if="draft.length && unmapped.length" class="unmapped">
      <li v-for="scene in unmapped" :key="scene">{{ t('music.rules.unmapped', { scene }) }}</li>
    </ul>
    <Button :label="t('music.rules.add')" icon="pi pi-plus" outlined class="add" :disabled="busy" @click="add" />
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import MultiSelect from 'primevue/multiselect'
import Select from 'primevue/select'
import SelectButton from 'primevue/selectbutton'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'

type Rule = Schemas['SceneRuleBody']
const props = defineProps<{
  rules: Rule[]
  playlists: Schemas['MusicPlaylist'][]
  /** The scenes OBS has now; empty while it is not connected. */
  obsScenes: string[]
}>()
const emit = defineEmits<{ saved: []; error: [unknown] }>()
const { t } = useI18n()

const draft = ref<Rule[]>([])
watch(() => props.rules, (rules) => (draft.value = structuredClone(rules)), { immediate: true })
const busy = ref(false)
const orders = computed(() => [
  { label: t('music.rules.inOrder'), value: false },
  { label: t('music.rules.shuffle'), value: true },
])

const lower = (s: string) => s.toLowerCase()
// OBS's scenes, and those rules name that OBS does not list now (it is not connected, or one was renamed).
const sceneOptions = computed(() => {
  const all = [...props.obsScenes]
  for (const s of props.rules.flatMap((r) => r.scenes)) if (!all.some((a) => lower(a) === lower(s))) all.push(s)
  return all
})
// The scenes no enabled rule plays on: switching to them stops the music.
const unmapped = computed(() => {
  const mapped = new Set(props.rules.filter((r) => r.enabled && r.playlist).flatMap((r) => r.scenes.map(lower)))
  return props.obsScenes.filter((s) => !mapped.has(lower(s)))
})

/** Saves every rule; a failure shows why and puts the saved rules back. */
async function save() {
  busy.value = true
  try {
    await api(client.PUT('/api/admin/music/scene-rules', { body: draft.value }))
    emit('saved')
  } catch (e) {
    draft.value = structuredClone(props.rules)
    emit('error', e)
  } finally {
    busy.value = false
  }
}

function add() {
  const playlist = props.playlists[0]?.id ?? null
  draft.value.push({
    id: '',
    name: t('music.rules.newName', { n: draft.value.length + 1 }),
    scenes: [],
    playlist,
    shuffle: false,
    volume: 50,
    autoplay: true,
    stop_on_leave: true,
    enabled: playlist !== null,
  })
  void save()
}

function remove(i: number) {
  draft.value.splice(i, 1)
  void save()
}
</script>

<style scoped>
.rules,
.unmapped {
  list-style: none;
  padding: 0;
  margin: 0;
}

.rule {
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

.wide {
  flex: 1 1 14rem;
}

.choices,
.choice,
.slider {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.choices {
  flex-wrap: wrap;
  gap: 0.5rem 1rem;
}

.slider input {
  accent-color: var(--p-primary-color);
}

.gone {
  color: var(--p-red-500);
}

.add {
  align-self: flex-start;
}
</style>
