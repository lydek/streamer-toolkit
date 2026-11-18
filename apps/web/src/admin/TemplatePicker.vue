<template>
  <section class="preview-section" aria-labelledby="preview-heading">
    <h2 id="preview-heading">{{ t('preview.heading') }}</h2>
    <p v-if="loadFailed" role="alert">{{ t('preview.loadFailed') }}</p>
    <p v-if="templateError" role="alert">{{ errorMessage(templateError) }}</p>
    <div class="options" role="radiogroup" aria-labelledby="preview-heading">
      <!-- Picking applies at once; clicks on a preview reach the radio because the iframe ignores the pointer. -->
      <label v-for="id in ids" :key="id" class="option">
        <span class="choice">
          <input
            type="radio"
            :name="`${overlay}-template`"
            :value="id"
            :checked="selected === id"
            @change="choose(id)"
          />
          {{ label(id) }}
        </span>
        <!-- Templates never render in the admin window. Each preview is the overlay in a sandbox without
             allow-same-origin: its origin is `null`, so it gets no IPC and no token, and data arrives by postMessage. -->
        <iframe
          ref="frames"
          class="preview"
          sandbox="allow-scripts"
          tabindex="-1"
          :src="`/overlay/${overlay}/?preview&template=${id}`"
          :title="label(id)"
          @load="post(start(), $event.target as HTMLIFrameElement)"
        ></iframe>
      </label>
    </div>
  </section>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import type { Incoming } from '../overlay/chat/chat'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast, useLiveEvents } from './home'

// The built-in template picker of an overlay's full page: one live preview per template.
const props = defineProps<{
  overlay: 'chat' | 'alerts'
  ids: string[]
  label: (id: string) => string
  /** What a preview starts from, posted when it loads and after each new snapshot. */
  start: () => Incoming[]
  /** Sees each live event before it is forwarded to the previews. */
  onLive?: (msg: Incoming) => void
}>()

const { t } = useI18n()
const toast = useToast()
const frames = ref<HTMLIFrameElement[]>([])
const selected = ref<string>()
const templateError = ref<string | null>(null)
const loadFailed = ref(false)

let choosing = false
let next: string | undefined

// The iframes' origin is `null`, which no targetOrigin can name; what goes in is public overlay data only.
const post = (msgs: Incoming[], frame?: HTMLIFrameElement) =>
  (frame ? [frame] : frames.value).forEach((f) => msgs.forEach((m) => f.contentWindow?.postMessage(m, '*')))

useLiveEvents(2000, (msg) => {
  props.onLive?.(msg)
  // Rebuilt rather than forwarded, so an empty overlay still shows its samples.
  if (msg.type === 'snapshot') post(props.start())
  else post([msg])
  if (msg.type === 'template.changed' && msg.payload.overlay === props.overlay) void refresh()
})

const read = () =>
  props.overlay === 'chat'
    ? api(client.GET('/api/admin/templates/chat'))
    : api(client.GET('/api/admin/templates/alerts'))

const save = (id: string) =>
  props.overlay === 'chat'
    ? api(client.PUT('/api/admin/templates/chat', { body: { selected: id as Schemas['ChatTemplate'] } }))
    : api(client.PUT('/api/admin/templates/alerts', { body: { selected: id as Schemas['AlertTemplate'] } }))

/** The selected template, and (chat only) why the user template is not in use: the overlay shows the default. */
async function refresh() {
  try {
    const status = await read()
    // A switch still in flight wins over an older answer.
    if (!choosing) selected.value = status.selected
    templateError.value = 'error' in status ? status.error : null
    loadFailed.value = false
  } catch {
    // Say so instead of showing no template picked; the next template change tries again.
    loadFailed.value = true
  }
}

/** Saves one pick at a time and then only the latest, so quick arrow-key moves cannot land out of order. */
async function choose(id: string) {
  selected.value = id
  next = id
  if (choosing) return
  choosing = true
  try {
    while (next) {
      const pick = next
      next = undefined
      try {
        await save(pick)
      } catch (e) {
        toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
      }
    }
  } finally {
    choosing = false
  }
  // Shows what the Host really has: the old template after a failure.
  await refresh()
}

onMounted(refresh)
</script>

<style scoped>
.preview-section {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.preview-section h2 {
  margin: 0;
}

.options {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(16rem, 100%), 1fr));
  gap: 1rem;
}

.option {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  cursor: pointer;
}

.choice {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.choice input {
  width: 1.25rem;
  height: 1.25rem;
  margin: 0;
  accent-color: var(--p-primary-color);
}

/* OBS browser sources are transparent; a plain dark box stands in for the scene. */
.preview {
  width: 100%;
  height: 14rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-950);
  pointer-events: none;
}

.option:has(input:checked) .preview {
  border-color: var(--p-primary-color);
  outline: 2px solid var(--p-primary-color);
}
</style>
