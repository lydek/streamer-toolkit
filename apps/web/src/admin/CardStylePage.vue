<template>
  <FullPage :title="t('pages.cardStyle.name')" tile="checkin" overlay="checkin">
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="!form">{{ t('app.loading') }}</p>
    <form v-else class="stack" @submit.prevent="save">
      <div class="field">
        <span id="cs-preset">{{ t('cardStyle.preset') }}</span>
        <SelectButton
          :model-value="form.preset"
          :options="PRESETS"
          :option-label="(p: string) => t(`cardStyle.presets.${p}`)"
          :allow-empty="false"
          aria-labelledby="cs-preset"
          @update:model-value="pickPreset"
        />
      </div>

      <fieldset class="group">
        <legend>{{ t('cardStyle.background') }}</legend>
        <div class="row">
          <label for="cs-bg-color">{{ t('cardStyle.color') }}</label>
          <input id="cs-bg-color" v-model="form.background_color" type="color" />
          <label for="cs-bg-image">{{ t('cardStyle.image') }}</label>
          <input id="cs-bg-image" type="file" :accept="IMAGE_ACCEPT" @change="upload($event, 'background_image')" />
          <template v-if="form.background_image">
            <img class="thumb" :src="`/checkin/${form.background_image}`" alt="" />
            <Button :label="t('cardStyle.removeImage')" text size="small" @click="form.background_image = null" />
          </template>
        </div>
      </fieldset>

      <fieldset class="group">
        <legend>{{ t('cardStyle.stamp') }}</legend>
        <div class="row">
          <label for="cs-stamp-color">{{ t('cardStyle.color') }}</label>
          <input id="cs-stamp-color" v-model="form.stamp_color" type="color" />
          <label for="cs-stamp-image">{{ t('cardStyle.image') }}</label>
          <input id="cs-stamp-image" type="file" :accept="IMAGE_ACCEPT" @change="upload($event, 'stamp_image')" />
          <template v-if="form.stamp_image">
            <img class="thumb" :src="`/checkin/${form.stamp_image}`" alt="" />
            <Button :label="t('cardStyle.removeImage')" text size="small" @click="form.stamp_image = null" />
          </template>
        </div>
        <div class="field">
          <label for="cs-per-row">{{ t('cardStyle.perRow') }}</label>
          <InputNumber v-model="form.per_row" input-id="cs-per-row" :min="1" :max="20" :allow-empty="false" show-buttons />
        </div>
      </fieldset>

      <div class="field">
        <label for="cs-title">{{ t('cardStyle.title') }}</label>
        <InputText id="cs-title" v-model="form.title" maxlength="100" aria-describedby="cs-vars" />
      </div>
      <div class="field">
        <label for="cs-description">{{ t('cardStyle.description') }}</label>
        <InputText id="cs-description" v-model="form.description" maxlength="100" aria-describedby="cs-vars" />
        <small id="cs-vars">{{ t('cardStyle.variables') }}</small>
      </div>

      <div class="field">
        <label for="cs-animation">{{ t('cardStyle.animation') }}</label>
        <Select
          v-model="form.animation"
          input-id="cs-animation"
          :options="ANIMATIONS"
          :option-label="(a: string) => t(`cardStyle.animations.${a}`)"
        />
      </div>
      <div class="check">
        <Checkbox v-model="form.show_repeat" input-id="cs-repeat" binary />
        <label for="cs-repeat">{{ t('cardStyle.showRepeat') }}</label>
      </div>

      <ul v-if="uploadError" role="alert" class="plain">
        <li>{{ uploadError }}</li>
      </ul>
      <div class="actions">
        <Button :label="t('cardStyle.test')" icon="pi pi-play" severity="secondary" :disabled="busy" @click="test" />
        <Button type="submit" :label="t('cardStyle.save')" :disabled="busy || !dirty" />
      </div>
    </form>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { importCardImage, IMAGE_ACCEPT, PRESET_COLORS } from './cardStyle'
import { onDrawerSubmit } from './drawer'
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
// The style as saved, to know whether the form has changes.
let saved = ''
const dirty = computed(() => !!form.value && JSON.stringify(form.value) !== saved)
const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))

async function load() {
  try {
    const style = await api(client.GET('/api/admin/checkin/card-style'))
    saved = JSON.stringify(style)
    form.value = style
  } catch (e) {
    failed.value = message(e)
  }
}

/** A preset brings its own colors, unless the colors were changed from the other preset's. */
function pickPreset(preset: Preset) {
  if (!form.value) return
  const before = PRESET_COLORS[form.value.preset as Preset]
  const untouched =
    before && form.value.background_color === before.background && form.value.stamp_color === before.stamp
  form.value.preset = preset
  if (untouched) {
    form.value.background_color = PRESET_COLORS[preset].background
    form.value.stamp_color = PRESET_COLORS[preset].stamp
  }
}

async function upload(e: Event, field: 'background_image' | 'stamp_image') {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file || !form.value) return
  uploadError.value = ''
  try {
    form.value[field] = (await importCardImage(file)).name
  } catch (err) {
    uploadError.value = t('cardStyle.uploadFailed', { file: file.name, reason: message(err) })
  }
  input.value = ''
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
.field {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.25rem;
}

.field :deep(.p-inputtext) {
  min-width: 20rem;
  max-width: 100%;
}

.group {
  display: grid;
  gap: 0.75rem;
  border: 1px solid var(--p-content-border-color);
  border-radius: 6px;
}

.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.thumb {
  width: 2.5rem;
  height: 2.5rem;
  object-fit: cover;
  border-radius: 4px;
}

.check {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.actions {
  display: flex;
  gap: 0.5rem;
}
</style>
