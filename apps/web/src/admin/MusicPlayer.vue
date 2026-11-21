<template>
  <div class="player stack">
    <p v-if="status && !status.onAir" role="status" class="off-air">
      <i class="pi pi-exclamation-triangle" aria-hidden="true"></i> {{ t('music.player.offAir') }}
    </p>
    <div class="field">
      <label for="player-playlist">{{ t('music.player.playlist') }}</label>
      <Select
        v-model="chosen"
        input-id="player-playlist"
        :options="playlists"
        option-label="name"
        option-value="id"
        :placeholder="t('music.player.choosePlaylist')"
      />
    </div>
    <p class="now">{{ status ? playerLine(status) : t('app.loading') }}</p>
    <div class="slider">
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
      <output for="player-position">{{ timeText }}</output>
    </div>
    <div class="controls">
      <Button
        icon="pi pi-step-backward"
        text
        :aria-label="t('music.player.previous')"
        :disabled="!track"
        @click="send({ action: 'previous' })"
      />
      <Button
        :icon="playing ? 'pi pi-pause' : 'pi pi-play'"
        :aria-label="t(playing ? 'music.player.pause' : 'music.player.play')"
        :disabled="!status || (!playing && !chosen)"
        @click="playing ? send({ action: 'pause' }) : send({ action: 'play', playlist: chosen })"
      />
      <Button icon="pi pi-step-forward" text :aria-label="t('music.player.next')" :disabled="!track" @click="send({ action: 'next' })" />
      <Button icon="pi pi-stop" text :aria-label="t('music.player.stop')" :disabled="!track" @click="send({ action: 'stop' })" />
    </div>
    <div class="choice">
      <Checkbox
        :model-value="status?.shuffle ?? false"
        input-id="player-shuffle"
        binary
        :disabled="!status"
        @update:model-value="(on: boolean) => saveOptions(status!.volume, on)"
      />
      <label for="player-shuffle">{{ t('music.player.shuffle') }}</label>
    </div>
    <!-- Dragging changes only the number shown; letting go saves it. -->
    <div class="volume">
      <label for="player-volume">{{ t('music.player.volume') }}</label>
      <span class="slider">
        <input
          id="player-volume"
          type="range"
          min="0"
          max="100"
          :value="volumeDraft ?? status?.volume ?? 0"
          :disabled="!status"
          @input="volumeDraft = Number(($event.target as HTMLInputElement).value)"
          @change="saveOptions(volumeDraft ?? status!.volume, status!.shuffle)"
        />
        <output for="player-volume">{{ volumeDraft ?? status?.volume ?? 0 }}%</output>
      </span>
    </div>
    <div class="volume">
      <label for="sound-volume">{{ t('music.player.soundVolume') }}</label>
      <span class="slider">
        <input
          id="sound-volume"
          type="range"
          min="0"
          max="100"
          :value="soundVolume ?? 0"
          :disabled="soundVolume === undefined"
          @input="soundVolume = Number(($event.target as HTMLInputElement).value)"
          @change="saveSoundVolume"
        />
        <output for="sound-volume">{{ soundVolume ?? 0 }}%</output>
      </span>
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
.field,
.volume {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.now,
.off-air {
  margin: 0;
}

.controls,
.choice,
.slider {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.slider {
  max-width: 24rem;
}

.slider input {
  flex: 1;
  min-width: 0;
  accent-color: var(--p-primary-color);
}
</style>
