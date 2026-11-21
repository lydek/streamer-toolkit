<template>
  <FullPage :title="t('pages.imageSizes.name')" tile="imageSizes">
    <div class="pickers">
      <div class="field">
        <span id="is-kind">{{ t('imageSizes.kind') }}</span>
        <SelectButton
          v-model="kind"
          :options="KINDS"
          :option-label="(k: ImageKind) => t(`imageSizes.kinds.${k}`)"
          :allow-empty="false"
          aria-labelledby="is-kind"
        />
      </div>
      <div class="field">
        <span id="is-fit">{{ t('imageSizes.fit') }}</span>
        <SelectButton
          v-model="fit"
          :options="FITS"
          :option-label="(f: Fit) => t(`imageSizes.fits.${f}`)"
          :allow-empty="false"
          aria-labelledby="is-fit"
        />
      </div>
      <Button :label="t('imageSizes.download')" icon="pi pi-download" :disabled="!sized || busy" @click="download" />
    </div>

    <label class="drop" :class="{ over }" @dragover.prevent="over = true" @dragleave="over = false" @drop.prevent="onDrop">
      <i class="pi pi-image" aria-hidden="true"></i>
      <span>{{ file ? file.name : t('imageSizes.drop') }}</span>
      <input type="file" :accept="IMAGE_SIZES_ACCEPT" class="sr-only" @change="onPick" />
    </label>

    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="busy">{{ t('app.loading') }}</p>
    <div v-else-if="sized" class="results">
      <div v-for="bg in BACKGROUNDS" :key="bg" class="sheet" :class="bg">
        <figure v-for="img in sized.images" :key="img.px">
          <img :src="img.data_url" :width="img.px" :height="img.px" :alt="t('imageSizes.alt', { px: img.px })" />
          <figcaption :class="{ over: img.too_large }">
            {{ t('imageSizes.size', { px: img.px, kb: (img.bytes / 1024).toFixed(1) }) }}
          </figcaption>
        </figure>
      </div>
      <p v-if="sized.images.some((i) => i.too_large)" class="over">{{ t('imageSizes.tooLarge') }}</p>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import SelectButton from 'primevue/selectbutton'
import { ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas } from './api'
import FullPage from './FullPage.vue'
import { type Fit, IMAGE_SIZES_ACCEPT, type ImageKind, makeSizes, sizesZip, zipName } from './imageSizes'
import { saveFile } from './save'

const KINDS: ImageKind[] = ['badge', 'points']
const FITS: Fit[] = ['pad', 'crop']
const BACKGROUNDS = ['dark', 'light']

const { t } = useI18n()
const kind = ref<ImageKind>('badge')
const fit = ref<Fit>('pad')
const file = shallowRef<File>()
const sized = shallowRef<Schemas['SizedImages']>()
const busy = ref(false)
const over = ref(false)
const failed = ref('')

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code) : String(e))
// Each request's number; only the latest one's answer (result, error, busy) shows.
let request = 0

async function run() {
  const picked = file.value
  if (!picked) return
  const mine = ++request
  busy.value = true
  failed.value = ''
  try {
    const result = await makeSizes(picked, kind.value, fit.value)
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
  void run()
}

function onPick(e: Event) {
  const input = e.target as HTMLInputElement
  choose(input.files?.[0])
  input.value = ''
}

function onDrop(e: DragEvent) {
  over.value = false
  choose(e.dataTransfer?.files[0])
}

watch([kind, fit], () => void run())

async function download() {
  const picked = file.value
  if (!picked) return
  try {
    await saveFile(zipName(picked.name, kind.value), { description: 'ZIP', mime: 'application/zip', extension: '.zip' }, () =>
      sizesZip(picked, kind.value, fit.value),
    )
  } catch (e) {
    failed.value = message(e)
  }
}
</script>

<style scoped>
.pickers {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.75rem 1.5rem;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.drop {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.5rem;
  max-width: 36rem;
  margin: 1rem 0;
  padding: 2rem 1rem;
  border: 2px dashed var(--p-content-border-color);
  border-radius: 8px;
  cursor: pointer;
}

.drop.over,
.drop:focus-within {
  border-color: var(--p-primary-color);
}

.results {
  display: grid;
  gap: 0.75rem;
}

.sheet {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 1.5rem;
  width: fit-content;
  padding: 1rem;
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

figure {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.25rem;
  margin: 0;
}

figcaption {
  font-size: 0.875rem;
}

.over {
  color: var(--p-red-500);
  font-weight: 600;
}
</style>
