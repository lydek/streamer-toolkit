<template>
  <FullPage tile="image-sizes" :status="t('imageSizes.summary')">
    <FormPanel class="setup-panel">
      <FormRow :label="t('imageSizes.kind')" label-id="is-kind">
        <SelectButton
          v-model="kind"
          :options="KINDS"
          :option-label="(k: ImageKind) => t(`imageSizes.kinds.${k}`)"
          :allow-empty="false"
          aria-labelledby="is-kind"
        />
        <template #hint>{{ t(`imageSizes.rules.${kind}`) }}</template>
      </FormRow>

      <FormRow :label="t('imageSizes.picture')" top>
        <div class="drop">
          <FilePick
            :drop-text="file ? file.name : t('imageSizes.drop')"
            :label="t('imageSizes.pick')"
            :hint="t('imageSizes.dropHint')"
            icon="pi pi-image"
            :accept="IMAGE_SIZES_ACCEPT"
            @pick="(files) => choose(files[0])"
          />
        </div>
      </FormRow>

      <FormRow :label="t('imageSizes.fit')" label-id="is-fit">
        <SelectButton
          v-model="fit"
          :options="FITS"
          :option-label="(f: Fit) => t(`imageSizes.fits.${f}`)"
          :allow-empty="false"
          aria-labelledby="is-fit"
        />
      </FormRow>

      <FormRow v-if="fit === 'manual' && source" :label="t('imageSizes.cropFields')" top>
        <div class="crop-wrap">
          <ImageCropper v-model="crop" :src="source.url" :width="source.width" :height="source.height" :ratio="ratio" @commit="run" />
          <!-- The same rectangle as numbers, in the picture's pixels; 等比例 keeps its width ÷ height. -->
          <fieldset class="crop-fields">
            <legend class="sr-only">{{ t('imageSizes.cropFields') }}</legend>
            <span v-for="f in FIELDS" :key="f" class="crop-field">
              <label :for="`crop-${f}`">{{ f === 'w' ? t('imageSizes.cropW') : f === 'h' ? t('imageSizes.cropH') : f.toUpperCase() }}</label>
              <InputNumber
                :input-id="`crop-${f}`"
                :model-value="crop[f]"
                :min="0"
                :use-grouping="false"
                input-class="crop-input"
                @update:model-value="(v: number | null) => setField(f, v)"
              />
            </span>
            <Button
              :label="t('imageSizes.cropLock')"
              :icon="ratio ? 'pi pi-lock' : 'pi pi-lock-open'"
              :aria-pressed="!!ratio"
              size="small"
              outlined
              @click="ratio = ratio ? undefined : crop.w / crop.h"
            />
            <Button :label="t('imageSizes.cropReset')" size="small" text @click="resetCrop" />
          </fieldset>
        </div>
      </FormRow>

      <!-- Padding: transparent, or a color the broadcaster picks (pad and a picked rectangle use it). -->
      <FormRow :label="t('imageSizes.bg')" label-id="is-bg">
        <div class="bg-row">
          <SelectButton
            v-model="bgMode"
            :options="BG_MODES"
            :option-label="(m: 'none' | 'color') => t(`imageSizes.bgs.${m}`)"
            :allow-empty="false"
            aria-labelledby="is-bg"
            @update:model-value="run"
          />
          <input
            v-if="bgMode === 'color'"
            v-model="bgColor"
            type="color"
            class="bg-color"
            :aria-label="t('imageSizes.bgColor')"
            @change="run"
          />
        </div>
      </FormRow>
    </FormPanel>

    <section class="results-section" aria-labelledby="out-h">
      <div class="sec-head">
        <h2 id="out-h">{{ t('imageSizes.resultsHeading') }}</h2>
        <span class="muted">{{ t('imageSizes.resultsHint') }}</span>
        <Button class="push" :label="t('imageSizes.download')" icon="pi pi-download" :disabled="!sized || busy" @click="download" />
      </div>

      <!-- The last results stay while new ones are made, so the page keeps its height and scroll position. -->
      <p v-if="failed" role="alert">{{ failed }}</p>
      <p v-else-if="busy && !sized">{{ t('app.loading') }}</p>
      <div v-if="sized && !failed" class="results" :class="{ updating: busy }" :aria-busy="busy">
        <div v-for="bg in BACKGROUNDS" :key="bg" class="sheet" :class="bg">
          <div class="sheet-head">
            <span class="sheet-label">{{ t(bg === 'dark' ? 'imageSizes.chatDark' : 'imageSizes.chatLight') }}</span>
            <!-- A points icon as Twitch shows it under the chat: the icon, then the balance. -->
            <span v-if="kind === 'points'" class="balance">
              <img :src="sized.images[0].data_url" alt="" width="20" height="20" />∞
            </span>
          </div>
          <div class="sheet-items">
            <figure v-for="img in sized.images" :key="img.px" class="size-item">
              <img :src="img.data_url" :width="img.px" :height="img.px" :alt="t('imageSizes.alt', { px: img.px })" />
              <figcaption :class="{ over: img.too_large }">
                {{ t('imageSizes.size', { px: img.px, kb: (img.bytes / 1024).toFixed(1) }) }}
              </figcaption>
              <button
                type="button"
                class="dl-one"
                :aria-label="t('imageSizes.downloadOne', { px: img.px })"
                @click="downloadOne(img)"
              >
                <i class="pi pi-download" aria-hidden="true"></i>
              </button>
            </figure>
          </div>
        </div>
        <p v-if="sized.images.some((i) => i.too_large)" class="over">{{ t('imageSizes.tooLarge') }}</p>
      </div>
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import SelectButton from 'primevue/selectbutton'
import { ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas } from './api'
import FilePick from './FilePick.vue'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import ImageCropper from './ImageCropper.vue'
import {
  type Crop,
  type Fit,
  IMAGE_SIZES_ACCEPT,
  IMAGE_SIZES_MAX_BYTES,
  type ImageKind,
  centeredCrop,
  makeSizes,
  setCropField,
  sizesZip,
  zipName,
} from './imageSizes'
import { saveFile } from './save'

const KINDS: ImageKind[] = ['badge', 'points']
const FITS: Fit[] = ['pad', 'crop', 'manual']
const BACKGROUNDS = ['dark', 'light']

const { t } = useI18n()
const kind = ref<ImageKind>('badge')
const fit = ref<Fit>('pad')
const file = shallowRef<File>()
const sized = shallowRef<Schemas['SizedImages']>()
const busy = ref(false)
const failed = ref('')
// The picture as shown under the frame, with its own size; the frame starts centered and as large as it fits.
const source = shallowRef<{ url: string; width: number; height: number }>()
const crop = ref<Crop>({ x: 0, y: 0, w: 1, h: 1 })
const BG_MODES: ('none' | 'color')[] = ['none', 'color']
const bgMode = ref<'none' | 'color'>('none')
const bgColor = ref('#1f2937')
const background = () => (bgMode.value === 'color' ? bgColor.value : null)
// Width ÷ height while 等比例 is on.
const ratio = ref<number>()
const FIELDS = ['x', 'y', 'w', 'h'] as const

function setField(field: keyof Crop, value: number | null) {
  if (value === null || !source.value) return
  crop.value = setCropField(crop.value, field, value, source.value.width, source.value.height, ratio.value)
  void run()
}

function resetCrop() {
  if (!source.value) return
  crop.value = centeredCrop(source.value.width, source.value.height)
  ratio.value = undefined
  void run()
}

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))
// Each request's number; only the latest one's answer (result, error, busy) shows.
let request = 0

async function run() {
  const picked = file.value
  if (!picked) return
  const mine = ++request
  // A picked square needs the picture's size first (the frame starts from it); until then nothing shows.
  if (fit.value === 'manual' && !source.value) {
    sized.value = undefined
    busy.value = false
    return
  }
  busy.value = true
  failed.value = ''
  try {
    const result = await makeSizes(picked, kind.value, fit.value, crop.value, background())
    if (mine === request) sized.value = result
  } catch (e) {
    if (mine !== request) return
    sized.value = undefined
    failed.value = message(e)
  } finally {
    if (mine === request) busy.value = false
  }
}

function choose(f: File | undefined) {
  if (!f) return
  file.value = f
  source.value = undefined
  // The last picture's sizes go at once, and an answer still on its way for it is dropped.
  sized.value = undefined
  failed.value = ''
  void run()
  // Too large: the Host would refuse it, so it is not read here either (run says why, except for a picked square).
  if (f.size > IMAGE_SIZES_MAX_BYTES) {
    if (fit.value === 'manual') failed.value = message(new ApiError('image_sizes.too_large'))
    return
  }
  // The frame needs the picture's own size; a picked square waits for it. A data: URL, since the admin window's
  // CSP allows no blob: images.
  const unreadable = () => {
    if (file.value === f && fit.value === 'manual') failed.value = message(new ApiError('image_sizes.failed'))
  }
  const reader = new FileReader()
  reader.onerror = unreadable
  reader.onload = () => {
    const url = String(reader.result)
    const img = new Image()
    img.onerror = unreadable
    img.onload = () => {
      if (file.value !== f) return
      source.value = { url, width: img.naturalWidth, height: img.naturalHeight }
      crop.value = centeredCrop(img.naturalWidth, img.naturalHeight)
      ratio.value = undefined
      if (fit.value === 'manual') void run()
    }
    img.src = url
  }
  reader.readAsDataURL(f)
}

watch([kind, fit], () => void run())

// The same save dialog as the ZIP; the PNG is already in the data URL, so it is decoded here instead of fetched.
async function downloadOne(img: { px: number; data_url: string }) {
  try {
    await saveFile(`${kind.value}-${img.px}px.png`, { description: 'PNG', mime: 'image/png', extension: '.png' }, async () => {
      const bytes = Uint8Array.from(atob(img.data_url.slice(img.data_url.indexOf(',') + 1)), (c) => c.charCodeAt(0))
      return new Blob([bytes], { type: 'image/png' })
    })
  } catch (e) {
    failed.value = message(e)
  }
}

async function download() {
  const picked = file.value
  if (!picked) return
  try {
    await saveFile(zipName(picked.name, kind.value), { description: 'ZIP', mime: 'application/zip', extension: '.zip' }, () =>
      sizesZip(picked, kind.value, fit.value, crop.value, background()),
    )
  } catch (e) {
    failed.value = message(e)
  }
}
</script>

<style scoped>
.setup-panel {
  width: 100%;
}

.drop {
  max-width: 36rem;
}

.bg-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.75rem;
}

.bg-color {
  width: 2.5rem;
  height: 2rem;
  padding: 0.125rem;
  border: 1px solid var(--p-surface-600);
  border-radius: 6px;
  background: none;
}

.crop-wrap {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.crop-fields {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
  margin: 0;
  padding: 0;
  border: 0;
}

.crop-field {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
}

.crop-field :deep(.crop-input) {
  width: 5.5rem;
}

.results-section {
  margin-top: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  max-width: 52rem;
}

.sec-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.sec-head h2 {
  margin: 0;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.push {
  margin-left: auto;
}

.results {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(22rem, 100%), 1fr));
  gap: 1rem;
}

.results.updating {
  opacity: 0.6;
}

.sheet {
  display: flex;
  flex-direction: column;
  gap: 0.875rem;
  padding: 1rem 1.125rem;
  border-radius: 8px;
}

.sheet.dark {
  background: #18181b;
  color: #f4f4f5;
}

.sheet.light {
  background: #f4f4f5;
  color: #18181b;
}

.sheet-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  font-size: 0.8125rem;
  opacity: 0.8;
}

.sheet-items {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 1.25rem;
}

.size-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.25rem;
  margin: 0;
}

figcaption {
  font-size: 0.8125rem;
  font-variant-numeric: tabular-nums;
}

.dl-one {
  color: inherit;
  background: none;
  border: 0;
  font: inherit;
  cursor: pointer;
  opacity: 0.75;
  padding: 0.125rem 0.25rem;
  border-radius: 4px;
}

.dl-one:hover {
  opacity: 1;
}

.over {
  color: var(--p-red-500);
  font-weight: 600;
  grid-column: 1 / -1;
  margin: 0;
}

.balance {
  display: inline-flex;
  gap: 0.25rem;
  align-items: center;
  font-weight: 600;
}
</style>
