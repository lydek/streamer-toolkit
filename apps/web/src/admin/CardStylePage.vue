<template>
  <FullPage tile="checkin" overlay="checkin" :status="t('cardStyle.summary')">
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!form">{{ t('app.loading') }}</p>
    <form v-else class="split" @submit.prevent="save">
      <FormPanel class="style-panel">
        <ul v-if="uploadError" role="alert" class="plain upload-alert">
          <li>{{ uploadError }}</li>
        </ul>
        <FormRow :label="t('cardStyle.preset')" label-id="cs-preset-label">
          <SelectButton
            :model-value="form.preset"
            :options="presetOptions"
            option-label="label"
            option-value="value"
            :allow-empty="false"
            aria-labelledby="cs-preset-label"
            @update:model-value="(v: string | null) => v && pickPreset(v)"
          />
        </FormRow>

        <FormRow :label="t('cardStyle.backgroundImage')">
          <div class="row">
            <FilePick
              input-id="cs-bg-image"
              :label="t('cardStyle.image')"
              :hint="t('cardStyle.imageHint')"
              icon="pi pi-image"
              :accept="IMAGE_ACCEPT"
              @pick="upload($event, 'background_image')"
            />
            <template v-if="form.background_image">
              <img class="thumb" :src="`/checkin/${form.background_image}`" alt="" />
              <Button :label="t('cardStyle.removeImage')" text size="small" @click="form.background_image = null" />
            </template>
            <span v-else class="muted">{{ t('cardStyle.solidColor') }}</span>
          </div>
        </FormRow>

        <FormRow :label="t('cardStyle.backgroundColor')" input-id="cs-bg-color">
          <input id="cs-bg-color" v-model="form.background_color" class="color-pick" type="color" />
        </FormRow>

        <FormRow :label="t('cardStyle.stampImage')">
          <div class="row">
            <FilePick
              input-id="cs-stamp-image"
              :label="t('cardStyle.image')"
              :hint="t('cardStyle.imageHint')"
              icon="pi pi-image"
              :accept="IMAGE_ACCEPT"
              @pick="upload($event, 'stamp_image')"
            />
            <template v-if="form.stamp_image">
              <img class="thumb" :src="`/checkin/${form.stamp_image}`" alt="" />
              <Button :label="t('cardStyle.removeImage')" text size="small" @click="form.stamp_image = null" />
            </template>
            <span v-else class="muted">{{ t('cardStyle.defaultStamp') }}</span>
          </div>
        </FormRow>

        <FormRow :label="t('cardStyle.stampColor')" input-id="cs-stamp-color">
          <input id="cs-stamp-color" v-model="form.stamp_color" class="color-pick" type="color" />
        </FormRow>

        <FormRow :label="t('cardStyle.perRow')" input-id="cs-per-row">
          <InputNumber v-model="form.per_row" input-id="cs-per-row" :min="1" :max="20" :allow-empty="false" show-buttons />
        </FormRow>

        <FormRow :label="t('cardStyle.title')" input-id="cs-title">
          <InputText id="cs-title" v-model="form.title" maxlength="100" aria-describedby="cs-vars" />
        </FormRow>

        <FormRow :label="t('cardStyle.description')" input-id="cs-description">
          <InputText id="cs-description" v-model="form.description" maxlength="100" aria-describedby="cs-vars" />
          <template #hint><span id="cs-vars">{{ t('cardStyle.variables') }}</span></template>
        </FormRow>

        <FormRow :label="t('cardStyle.animation')" input-id="cs-animation">
          <Select
            v-model="form.animation"
            input-id="cs-animation"
            :options="ANIMATIONS"
            :option-label="(a: string) => t(`cardStyle.animations.${a}`)"
          />
        </FormRow>

        <FormRow :label="t('cardStyle.repeatHeading')" label-id="cs-repeat-label">
          <SelectButton
            v-model="form.show_repeat"
            :options="repeatOptions"
            option-label="label"
            option-value="value"
            :allow-empty="false"
            aria-labelledby="cs-repeat-label"
          />
        </FormRow>

        <template #foot>
          <section class="pack-sec" aria-labelledby="cs-packs-heading">
            <div class="pack-head">
              <h3 id="cs-packs-heading">{{ t('cardStyle.packs.heading') }}</h3>
              <span class="muted">{{ t('cardStyle.packs.sandboxNote') }}</span>
              <div class="pack-upload">
                <FilePick
                  input-id="cs-pack"
                  :label="t('cardStyle.packs.upload')"
                  :hint="t('cardStyle.packs.uploadHint')"
                  accept=".zip,application/zip"
                  :disabled="busy"
                  @pick="uploadPack"
                />
              </div>
            </div>
            <ul v-if="packs.length" class="plain pack-list">
              <li v-for="p in packs" :key="p.id">
                <span class="pack-name">{{ p.name }}</span>
                <Tag v-if="form.preset === `pack:${p.id}`" :value="t('cardStyle.packs.inUse')" severity="success" />
                <Button
                  class="pack-del"
                  icon="pi pi-trash"
                  text
                  severity="danger"
                  :aria-label="t('cardStyle.packs.delete', { name: p.name })"
                  @click="deletePack(p)"
                />
              </li>
            </ul>
          </section>
        </template>
      </FormPanel>

      <div class="preview-col">
        <CardPreview :style="form" :slots="slots" />
        <div class="actions">
          <Button type="submit" :label="t('cardStyle.save')" :disabled="busy || !dirty" />
          <Button :label="t('cardStyle.test')" icon="pi pi-play" severity="secondary" :disabled="busy" @click="test" />
        </div>
      </div>
    </form>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import SelectButton from 'primevue/selectbutton'
import Tag from 'primevue/tag'
import { useToast } from 'primevue/usetoast'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import CardPreview from './CardPreview.vue'
import { importCardImage, IMAGE_ACCEPT, PRESET_COLORS, uploadCardPack } from './cardStyle'
import { onDrawerSubmit } from './drawer'
import FilePick from './FilePick.vue'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'

type Style = Schemas['CardStyleBody']
type Preset = keyof typeof PRESET_COLORS
const PRESETS: Preset[] = ['simple', 'journal']
const ANIMATIONS = ['slide', 'fade', 'pop', 'none']

const { t } = useI18n()
const toast = useToast()
const form = ref<Style>()
const failed = ref('')
const uploadError = ref('')
const busy = ref(false)
const slots = ref(10)
const packs = ref<Schemas['CardPack'][]>([])
// The built-in presets, then each installed pack.
const presetOptions = computed(() => [
  ...PRESETS.map((p) => ({ value: p as string, label: t(`cardStyle.presets.${p}`) })),
  ...packs.value.map((p) => ({ value: `pack:${p.id}`, label: p.name })),
])
const repeatOptions = computed(() => [
  { value: true, label: t('cardStyle.repeatShow') },
  { value: false, label: t('cardStyle.repeatHide') },
])
// The style as saved, to know whether the form has changes.
let saved = ''
const dirty = computed(() => !!form.value && JSON.stringify(form.value) !== saved)
const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))

async function load() {
  try {
    const [style, settings, installed] = await Promise.all([
      api(client.GET('/api/admin/checkin/card-style')),
      api(client.GET('/api/admin/checkin/settings')),
      api(client.GET('/api/admin/checkin/packs')),
    ])
    slots.value = settings.card_slots
    packs.value = installed
    saved = JSON.stringify(style)
    form.value = style
  } catch (e) {
    failed.value = message(e)
  }
}

/** A preset brings its own colors, unless the colors were changed from the other preset's. */
function pickPreset(preset: string) {
  if (!form.value) return
  const before = PRESET_COLORS[form.value.preset as Preset]
  const after = PRESET_COLORS[preset as Preset]
  const untouched =
    before && form.value.background_color === before.background && form.value.stamp_color === before.stamp
  form.value.preset = preset
  if (untouched && after) {
    form.value.background_color = after.background
    form.value.stamp_color = after.stamp
  }
}

/** Installs a pack; a refused zip says which file (and where in index.html). */
async function uploadPack(files: File[]) {
  const file = files[0]
  if (!file) return
  uploadError.value = ''
  try {
    const pack = await uploadCardPack(file)
    packs.value = [...packs.value, pack].sort((a, b) => a.name.localeCompare(b.name))
    toast.add(successToast(t('cardStyle.packs.installed', { name: pack.name })))
  } catch (err) {
    uploadError.value = t('cardStyle.uploadFailed', { file: file.name, reason: message(err) })
  }
}

async function deletePack(p: Schemas['CardPack']) {
  try {
    await api(client.DELETE('/api/admin/checkin/packs/{id}', { params: { path: { id: p.id } } }))
    packs.value = packs.value.filter((x) => x.id !== p.id)
    // The style falls back to the simple one on screen; pick it here too so saving stays valid.
    if (form.value?.preset === `pack:${p.id}`) pickPreset('simple')
  } catch (err) {
    toast.add(errorToast(message(err)))
  }
}

async function upload(files: File[], field: 'background_image' | 'stamp_image') {
  const file = files[0]
  if (!file || !form.value) return
  uploadError.value = ''
  try {
    form.value[field] = (await importCardImage(file)).name
  } catch (err) {
    uploadError.value = t('cardStyle.uploadFailed', { file: file.name, reason: message(err) })
  }
}

/** Saves the form; leaving the page saves it too, and a failed save keeps the page. */
async function save(): Promise<boolean> {
  if (!form.value || !dirty.value) return true
  busy.value = true
  try {
    const style = await api(client.PUT('/api/admin/checkin/card-style', { body: form.value }))
    saved = JSON.stringify(style)
    form.value = style
    toast.add(successToast(t('cardStyle.saved')))
    return true
  } catch (e) {
    toast.add(errorToast(message(e)))
    return false
  } finally {
    busy.value = false
  }
}
onDrawerSubmit(save)

/** 在 OBS 試播 shows the saved style, so unsaved changes are saved first. */
async function test() {
  if (!(await save())) return
  try {
    await api(client.POST('/api/admin/checkin/card-style/test'))
  } catch (e) {
    toast.add(errorToast(message(e)))
  }
}

void load()
</script>

<style scoped>
.split {
  display: grid;
  grid-template-columns: minmax(0, 38rem) minmax(0, 1fr);
  gap: 1.5rem;
  align-items: start;
  max-width: none;
}

@container view (max-width: 60rem) {
  .split {
    grid-template-columns: minmax(0, 1fr);
  }
}

.style-panel {
  max-width: none;
  width: 100%;
}

.upload-alert {
  padding: 0.5rem 0;
  color: var(--p-red-400);
}

.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.color-pick {
  width: 4.5rem;
  height: 2.25rem;
  padding: 0.125rem;
  border: 1px solid var(--p-content-border-color);
  border-radius: 6px;
  background: var(--p-surface-950);
  cursor: pointer;
}

.thumb {
  width: 2.5rem;
  height: 2.5rem;
  object-fit: cover;
  border-radius: 4px;
}

.muted,
.hint {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.pack-sec {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  width: 100%;
}

.pack-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.pack-head h3 {
  margin: 0;
  font-size: 1rem;
}

.pack-upload {
  margin-left: auto;
}

.plain {
  list-style: none;
  margin: 0;
  padding: 0;
}

.pack-list li {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.375rem 0;
  border-top: 1px solid var(--p-surface-800);
}

.pack-name {
  font-weight: 600;
}

.pack-del {
  margin-left: auto;
}

.preview-col {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  min-width: 0;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
</style>
