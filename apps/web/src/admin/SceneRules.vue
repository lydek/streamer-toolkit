<template>
  <div class="rules-root">
    <p v-if="!obsScenes.length" class="muted notice">{{ t('scene.notConnected') }}</p>

    <div class="tbl-wrap">
      <table v-if="draft.length" class="table">
        <thead>
          <tr>
            <th scope="col">{{ t('music.rules.name') }}</th>
            <th scope="col">{{ t('music.rules.scenes') }}</th>
            <th scope="col">{{ t('music.rules.playlist') }}</th>
            <th scope="col">{{ t('music.rules.order') }}</th>
            <th scope="col" class="num">{{ t('music.rules.volume') }}</th>
            <th scope="col">{{ t('music.rules.autoplay') }}</th>
            <th scope="col">{{ t('music.rules.stopOnLeave') }}</th>
            <th scope="col" class="num">{{ t('music.rules.enabled') }}</th>
            <th scope="col"><span class="sr-only">{{ t('screenTools.media.actions') }}</span></th>
          </tr>
        </thead>
        <tbody v-for="(rule, i) in draft" :key="rule.id || i" class="rule-group">
          <tr>
            <th scope="row">
              <b>{{ rule.name }}</b>
            </th>
            <td>
              <span v-if="rule.scenes.length" class="chips">
                <Tag v-for="s in rule.scenes" :key="s" :value="s" severity="secondary" />
              </span>
              <span v-else class="muted">{{ t('music.rules.noScenes') }}</span>
            </td>
            <td>
              <span v-if="!rule.playlist" class="gone">{{ t('music.rules.playlistGone') }}</span>
              <span v-else>{{ playlistLabel(rule.playlist) }}</span>
            </td>
            <td>{{ t(rule.shuffle ? 'music.rules.shuffle' : 'music.rules.inOrder') }}</td>
            <td class="num">{{ rule.volume }}%</td>
            <td>
              <ToggleSwitch
                v-model="rule.autoplay"
                :input-id="`rule-autoplay-${i}`"
                :aria-label="`${t('music.rules.autoplay')}：${rule.name}`"
                @update:model-value="save"
              />
            </td>
            <td>
              <ToggleSwitch
                v-model="rule.stop_on_leave"
                :input-id="`rule-leave-${i}`"
                :aria-label="`${t('music.rules.stopOnLeave')}：${rule.name}`"
                @update:model-value="save"
              />
            </td>
            <td class="num">
              <ToggleSwitch
                v-model="rule.enabled"
                :input-id="`rule-enabled-${i}`"
                :disabled="!rule.playlist"
                :aria-label="`${t('music.rules.enabled')}：${rule.name}`"
                @update:model-value="save"
              />
            </td>
            <td class="end">
              <Button
                icon="pi pi-pencil"
                text
                size="small"
                :aria-label="t('music.rules.editFor', { name: rule.name })"
                :aria-expanded="editing === i"
                :aria-controls="`rule-edit-${i}`"
                @click="toggleEdit(i)"
              />
            </td>
          </tr>
          <tr v-show="editing === i" :id="`rule-edit-${i}`" class="edit-row">
            <td colspan="9">
              <div class="edit-grid">
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
                <Button
                  :label="t('music.rules.delete')"
                  icon="pi pi-trash"
                  severity="danger"
                  text
                  @click="remove(i)"
                />
              </div>
            </td>
          </tr>
        </tbody>
      </table>

      <div class="tbl-foot">
        <span class="muted">
          <template v-if="draft.length && unmapped.length">
            {{ unmapped.map((scene) => t('music.rules.unmapped', { scene })).join(' · ') }}
          </template>
        </span>
        <Button :label="t('music.rules.add')" icon="pi pi-plus" text :disabled="busy" @click="add" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import MultiSelect from 'primevue/multiselect'
import Select from 'primevue/select'
import SelectButton from 'primevue/selectbutton'
import Tag from 'primevue/tag'
import ToggleSwitch from 'primevue/toggleswitch'
import { computed, nextTick, ref, watch } from 'vue'
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
const editing = ref<number | null>(null)
watch(() => props.rules, (rules) => (draft.value = structuredClone(rules)), { immediate: true })
const busy = ref(false)
let saveQueue: Promise<unknown> = Promise.resolve()
let pendingSaves = 0

async function toggleEdit(i: number) {
  editing.value = editing.value === i ? null : i
  if (editing.value === i) {
    await nextTick()
    document.getElementById(`rule-name-${i}`)?.focus()
  }
}
const orders = computed(() => [
  { label: t('music.rules.inOrder'), value: false },
  { label: t('music.rules.shuffle'), value: true },
])

function playlistLabel(id: string | null) {
  const p = props.playlists.find((x) => x.id === id)
  return p ? `${p.name}（${t('music.count', { n: p.tracks.length })}）` : t('music.rules.playlistGone')
}

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

/** Saves every rule one at a time so rapid switches never overwrite newer changes. */
function save() {
  const snapshot = draft.value.map((r) => ({ ...r, scenes: [...r.scenes] }))
  pendingSaves++
  busy.value = true
  saveQueue = saveQueue.then(async () => {
    try {
      await api(client.PUT('/api/admin/music/scene-rules', { body: snapshot }))
      if (pendingSaves === 1) emit('saved')
    } catch (e) {
      if (pendingSaves === 1) draft.value = structuredClone(props.rules)
      emit('error', e)
    } finally {
      pendingSaves--
      if (pendingSaves === 0) busy.value = false
    }
  })
  return saveQueue
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
  editing.value = draft.value.length - 1
  void save()
}

function remove(i: number) {
  draft.value.splice(i, 1)
  if (editing.value === i) editing.value = null
  void save()
}
</script>

<style scoped>
.rules-root {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.notice {
  margin: 0;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.chips {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 0.25rem;
}

.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.end {
  text-align: right;
  white-space: nowrap;
}

.gone {
  color: var(--p-red-400);
}

.edit-grid {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.75rem 1rem;
  padding: 0.5rem 0;
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

.slider {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.slider input {
  accent-color: var(--p-primary-color);
}
</style>
