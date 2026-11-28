<template>
  <div class="player-card">
    <p v-if="status && !status.onAir" role="status" class="off-air">
      <i class="pi pi-exclamation-triangle" aria-hidden="true"></i> {{ t('music.player.offAir') }}
    </p>

    <div class="player">
      <div class="cover tint-yellow" aria-hidden="true">
        <i class="pi pi-headphones"></i>
      </div>

      <div class="pl-main">
        <div class="pl-head">
          <b class="now">{{ status ? playerLine(status) : t('app.loading') }}</b>
          <div class="pl-pick">
            <Select
              v-model="chosen"
              input-id="player-playlist"
              :options="playlists"
              option-label="name"
              option-value="id"
              :placeholder="t('music.player.choosePlaylist')"
              :aria-label="t('music.player.playlist')"
            />
            <span class="choice">
              <Checkbox
                :model-value="status?.shuffle ?? false"
                input-id="player-shuffle"
                binary
                :disabled="!status"
                @update:model-value="(on: boolean) => saveOptions(status!.volume, on)"
              />
              <label for="player-shuffle">{{ t('music.player.shuffle') }}</label>
            </span>
          </div>
        </div>

        <div class="slider prog">
          <input
            id="player-position"
            type="range"
            min="0"
            :max="track?.durationMs ?? 0"
            step="1000"
            :value="seekDraft ?? position"
            :disabled="!track"
            :aria-label="t('music.player.position')"
            :aria-valuetext="timeText"
            @input="seekDraft = Number(($event.target as HTMLInputElement).value)"
            @change="seek"
          />
          <output for="player-position" class="time">{{ timeText }}</output>
        </div>
      </div>

      <div class="controls">
        <Button
          icon="pi pi-step-backward"
          text
          rounded
          :aria-label="t('music.player.previous')"
          :disabled="!track"
          @click="send({ action: 'previous' })"
        />
        <Button
          :icon="playing ? 'pi pi-pause' : 'pi pi-play'"
          rounded
          class="play-big"
          :aria-label="t(playing ? 'music.player.pause' : 'music.player.play')"
          :disabled="!status || (!playing && !chosen)"
          @click="playing ? send({ action: 'pause' }) : send({ action: 'play', playlist: chosen })"
        />
        <Button
          icon="pi pi-step-forward"
          text
          rounded
          :aria-label="t('music.player.next')"
          :disabled="!track"
          @click="send({ action: 'next' })"
        />
        <Button
          icon="pi pi-stop"
          text
          rounded
          :aria-label="t('music.player.stop')"
          :disabled="!track"
          @click="send({ action: 'stop' })"
        />
      </div>

      <!-- Dragging changes only the number shown; letting go saves it. -->
      <div class="pl-vol">
        <div class="vol-row">
          <label for="player-volume">{{ t('music.player.volumeShort') }}</label>
          <input
            id="player-volume"
            type="range"
            min="0"
            max="100"
            :value="volumeDraft ?? status?.volume ?? 0"
            :disabled="!status"
            :aria-label="t('music.player.volume')"
            @input="volumeDraft = Number(($event.target as HTMLInputElement).value)"
            @change="saveOptions(volumeDraft ?? status!.volume, status!.shuffle)"
          />
          <output for="player-volume">{{ volumeDraft ?? status?.volume ?? 0 }}%</output>
        </div>
        <div class="vol-row">
          <label for="sound-volume">{{ t('music.player.soundVolumeShort') }}</label>
          <input
            id="sound-volume"
            type="range"
            min="0"
            max="100"
            :value="soundVolume ?? 0"
            :disabled="soundVolume === undefined"
            :aria-label="t('music.player.soundVolume')"
            @input="soundVolume = Number(($event.target as HTMLInputElement).value)"
            @change="saveSoundVolume"
          />
          <output for="sound-volume">{{ soundVolume ?? 0 }}%</output>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import Select from 'primevue/select'
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { positionMs } from '../overlay/bgm/player'
import { type Schemas, api, client } from './api'
import { formatLength, playerLine } from './music'

const props = defineProps<{
  /** The latest `bgm.player` event; `undefined` until the live connection has it. */
  event?: Schemas['BgmPlayerEvent']
  playlists: Schemas['MusicPlaylist'][]
}>()
const emit = defineEmits<{ error: [e: unknown] }>()
const { t } = useI18n()

const status = computed(() => props.event?.payload)
const track = computed(() => status.value?.track ?? undefined)
const playing = computed(() => status.value?.state === 'playing')
const chosen = ref<string>()
// Follows the playlist started anywhere, the home drawer included.
watch(() => status.value?.playlist, (p) => p && (chosen.value = p), { immediate: true })

const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 500)
onBeforeUnmount(() => clearInterval(ticker))
const position = computed(() => (props.event ? positionMs(props.event.payload, props.event.occurredAt, now.value) : 0))
const seekDraft = ref<number>()
const timeText = computed(() => `${formatLength(seekDraft.value ?? position.value)} / ${formatLength(track.value?.durationMs ?? 0)}`)

const volumeDraft = ref<number>()
// The saved volume has arrived: show it again.
watch(() => status.value?.volume, () => (volumeDraft.value = undefined))
const soundVolume = ref<number>()
api(client.GET('/api/admin/sounds')).then(
  (s) => (soundVolume.value = s.volume),
  (e) => emit('error', e),
)

async function send(body: Schemas['PlayerControl']) {
  try {
    await api(client.POST('/api/admin/music/player', { body }))
  } catch (e) {
    emit('error', e)
  }
}

async function seek() {
  const position_ms = seekDraft.value
  seekDraft.value = undefined
  if (position_ms !== undefined) await send({ action: 'seek', position_ms })
}

async function saveOptions(volume: number, shuffle: boolean) {
  try {
    await api(client.PUT('/api/admin/music/player/options', { body: { volume, shuffle } }))
  } catch (e) {
    volumeDraft.value = undefined
    emit('error', e)
  }
}

async function saveSoundVolume() {
  try {
    await api(client.PUT('/api/admin/sounds/volume', { body: { volume: soundVolume.value ?? 100 } }))
  } catch (e) {
    emit('error', e)
  }
}
</script>

<style scoped>
.player-card {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 1rem 1.25rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.off-air {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin: 0;
  color: var(--p-yellow-300);
  font-size: 0.875rem;
}

.player {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 1rem 1.25rem;
}

.cover {
  display: grid;
  place-items: center;
  width: 3rem;
  height: 3rem;
  border-radius: 6px;
  font-size: 1.25rem;
  flex-shrink: 0;
}

.pl-main {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  flex: 1 1 16rem;
  min-width: 0;
}

.pl-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.now {
  font-size: 1rem;
  overflow-wrap: anywhere;
}

.pl-pick {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.pl-pick :deep(.p-select) {
  min-width: 0;
  max-width: 14rem;
}

.controls,
.choice,
.slider {
  display: flex;
  align-items: center;
  gap: 0.375rem;
}

.slider input,
.vol-row input {
  flex: 1;
  min-width: 0;
  accent-color: var(--p-primary-color);
}

.time,
.vol-row output {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.pl-vol {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
  flex: 0 1 15rem;
  min-width: 12rem;
}

.vol-row {
  display: grid;
  grid-template-columns: 4.5rem minmax(4rem, 1fr) 2.75rem;
  align-items: center;
  gap: 0.5rem;
}

.vol-row label {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.vol-row output {
  text-align: right;
}

/* The prototype's big yellow play button; dark icon on yellow for contrast (WCAG 1.4.11 and 1.4.3). */
.play-big {
  width: 2.75rem;
  height: 2.75rem;
  border-color: var(--p-yellow-400);
  background: var(--p-yellow-400);
  color: #2a2205;
}

.play-big:not(:disabled):hover {
  border-color: var(--p-yellow-300);
  background: var(--p-yellow-300);
  color: #2a2205;
}
</style>
