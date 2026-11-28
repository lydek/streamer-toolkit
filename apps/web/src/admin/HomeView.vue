<template>
  <Teleport to="#skip-links" :disabled="!skipLinks">
    <a href="#tiles" class="skip-link" @click.prevent="focusTiles">{{ t('home.skip') }}</a>
  </Teleport>
  <div class="layout">
    <main>
      <header ref="topEl" class="top">
        <!-- The app bar shows the name; the status lights and settings live there too. -->
        <h1 class="sr-only">{{ t('app.name') }}</h1>
        <div class="stage-switch">
          <h2 id="stage-heading" class="sr-only">{{ t('stage.title') }}</h2>
          <SelectButton
            v-model="shownStage"
            :options="STAGES"
            :option-label="(s: Stage) => t(`stage.${s}`)"
            :allow-empty="false"
            aria-labelledby="stage-heading"
          >
            <template #option="{ option }">
              <i :class="STAGE_ICONS[option as Stage]" aria-hidden="true"></i>
              <span>{{ t(`stage.${option}`) }}</span>
            </template>
          </SelectButton>
          <template v-if="stage.manual.value">
            <span class="manual">{{ t('stage.manual') }}</span>
            <Button :label="t('stage.backToAuto')" size="small" text @click="stage.backToAuto()" />
          </template>
          <!-- 下播後: show the numbers, mask them, or fold the whole home away (no number anxiety). -->
          <template v-if="shownStage === 'after'">
            <span id="numbers-heading" class="sr-only">{{ t('home.numbers.label') }}</span>
            <SelectButton
              :model-value="numbers.mode"
              :options="NUMBER_MODES"
              :option-label="(m: NumbersMode) => t(`home.numbers.${m}`)"
              :allow-empty="false"
              aria-labelledby="numbers-heading"
              @update:model-value="(mode: NumbersMode) => saveNumbers({ mode })"
            />
          </template>
        </div>
        <div class="lights">
          <Button v-if="arranging" ref="doneButton" :label="t('arrange.done')" icon="pi pi-check" @click="leaveArrange" />
          <Button
            v-else
            ref="arrangeButton"
            :label="t('home.arrange')"
            icon="pi pi-pencil"
            severity="secondary"
            outlined
            :disabled="!modules"
            @click="enterArrange"
          />
        </div>
      </header>
      <p v-if="templateError" role="alert">{{ errorMessage(templateError) }}</p>
      <UpdateNotice @dismissed="keepFocus(firstTile)" />

      <section id="tiles" ref="tilesEl" tabindex="-1" :aria-label="t('home.tiles')">
        <p v-if="!modules">{{ loadFailed ? t('home.loadFailed') : t('app.loading') }}</p>
        <!-- One per stage: a save still running for another stage cannot move focus or announce here. -->
        <ArrangeTiles
          v-else-if="arranging"
          :key="shownStage"
          :saved="savedTiles[shownStage]"
          :defaults="stageDefaults(shownStage)"
          :save="saveTiles"
          :modules="modules"
          :name="moduleName"
          :lines="lines"
        />
        <div v-else-if="folded" class="block folded">
          <p>{{ t('home.numbers.folded') }}</p>
          <Button :label="t('home.numbers.peek')" outlined @click="peek = true" />
        </div>
        <div v-else-if="!tiles.length" class="stack">
          <p>{{ t('home.empty') }}</p>
          <Button :label="t('home.arrange')" icon="pi pi-pencil" outlined data-drawer-arrange @click="enterArrange" />
        </div>
        <ul v-else class="plain tiles">
          <li v-for="tile in tiles" :key="tile.id" :class="sizeOf(tile) && `size-${sizeOf(tile)}`">
            <!-- Things to fix, each linking to the page that fixes it; the block shows only when it has some. -->
            <section v-if="tile.id === 'pending'" class="block" aria-labelledby="pending-heading">
              <h2 id="pending-heading">{{ t('home.blocks.pending') }}</h2>
              <ul class="plain">
                <li v-for="p in pending" :key="p.id">
                  <button v-if="p.drawer" type="button" class="link-button" @click="toggle(p.drawer, $event.currentTarget)">{{ p.text }}</button>
                  <RouterLink v-else :to="p.to!">{{ p.text }}</RouterLink>
                </li>
              </ul>
            </section>
            <StageCard v-else-if="tile.id === 'checklist'" part="checklist" :stage="stage" />
            <StageCard v-else-if="tile.id === 'summary'" part="session" :stage="stage" :masked="maskedNumber" @reveal="revealNumber" />
            <!-- 本場直播 and 事件流: a small tile that opens in place into its block, and back. -->
            <HomeTile
              v-else-if="tile.id in EXPANDS && !opened.has(tile.id)"
              :id="tile.id"
              :name="moduleName(tile.id)"
              :icon="tile.icon"
              :tint="tile.tint"
              :line="lines[tile.id] ?? { state: 'loading' }"
              :expanded="false"
              :controls="`block-${tile.id}`"
              :data-expand="tile.id"
              :immediate="tile.id === 'stats' && shownStage === 'after'"
              @click="openBlock(tile.id)"
            />
            <StageCard v-else-if="tile.id === 'stats'" id="block-stats" part="session" :stage="stage" :masked="maskedNumber" @reveal="revealNumber">
              <template #actions><BlockClose :name="moduleName('stats')" expand-id="stats" @close="closeBlock('stats')" /></template>
            </StageCard>
            <section v-else-if="tile.id === 'feed'" id="block-feed" class="block" aria-labelledby="feed-heading">
              <div class="block-head">
                <h2 id="feed-heading">{{ moduleName('feed') }}</h2>
                <BlockClose :name="moduleName('feed')" expand-id="feed" @close="closeBlock('feed')" />
              </div>
              <p v-if="feed.state === 'failed'" role="alert">{{ t('feed.loadFailed') }}</p>
              <p v-else-if="feed.state === 'loading'">{{ t('app.loading') }}</p>
              <template v-else-if="feed.state === 'ready'">
                <p v-if="!feed.items.length">{{ t('feed.empty') }}</p>
                <ol v-else class="plain feed-list">
                  <li v-for="(item, i) in feed.items" :key="i">
                    <time :datetime="item.at">{{ formatTime(item.at) }}</time>
                    <span class="feed-text">
                      {{ feedText(item) }}
                      <span v-if="item.simulated" class="tag">{{ t('feed.test') }}</span>
                    </span>
                    <span v-if="item.result" class="feed-result">{{ feedResult(item) }}</span>
                  </li>
                </ol>
              </template>
            </section>
            <HomeTile
              v-else
              :id="tile.id"
              :name="moduleName(tile.id)"
              :icon="tile.icon"
              :tint="tile.tint"
              :line="lines[tile.id] ?? { state: 'loading' }"
              :attention="attention[tile.id]"
              :unbuilt="!!tile.unbuilt"
              :expanded="drawer.current.value === tile.id"
              :masked="maskedNumber(tile.id)"
              :immediate="tile.id === 'mixer'"
              @click="!tile.unbuilt && toggle(tile.id, $event.currentTarget)"
            >
              <!-- A masked number has a button that shows just that one. -->
              <template v-if="maskedNumber(tile.id)" #actions>
                <Button
                  :label="t('home.numbers.reveal')"
                  :aria-label="`${t('home.numbers.reveal')}：${moduleName(tile.id)}`"
                  size="small"
                  text
                  @click="revealNumber(tile.id)"
                />
              </template>
              <!-- A few tiles act in place: the first counter, the first three sounds, the break card. -->
              <template v-else-if="tile.id === 'counter' && live.counters?.length" #actions>
                <Button
                  v-for="d in [-1, 1] as const"
                  :key="d"
                  :label="d < 0 ? '−1' : '+1'"
                  :aria-label="`${t(d < 0 ? 'counter.minusOne' : 'counter.plusOne')}：${live.counters[0].name}`"
                  size="small"
                  outlined
                  @click="tileAction(stepCounter(live.counters[0].id, d))"
                />
              </template>
              <template v-else-if="tile.id === 'sound' && sounds.state === 'ready' && sounds.items.length" #actions>
                <Button v-for="s in sounds.items.slice(0, 3)" :key="s.id" :label="s.label" size="small" outlined @click="tileAction(testSound(s.id))" />
              </template>
              <!-- The chosen mic's mute; disabled, with the reason beside it, while it cannot be used. -->
              <template v-else-if="tile.id === 'mixer'" #actions>
                <Button
                  :label="t(micState.input?.muted ? 'mixer.tile.unmute' : 'mixer.tile.mute')"
                  :icon="micState.input?.muted ? 'pi pi-microphone' : 'pi pi-volume-off'"
                  :aria-pressed="!!micState.input?.muted"
                  :disabled="!micState.input || micBusy"
                  :aria-describedby="micState.reason ? 'mic-reason' : undefined"
                  size="small"
                  outlined
                  @click="toggleMic"
                />
                <small v-if="micState.reason" id="mic-reason" class="tile-reason">{{ micState.reason }}</small>
              </template>
              <template v-else-if="tile.id === 'screen-tools' && live.breakCard" #actions>
                <ToggleSwitch
                  input-id="tile-break"
                  :model-value="live.breakCard.shown"
                  @update:model-value="(on: boolean) => tileAction(on ? showBreak(breakAgain(live.breakCard!)) : hideBreak())"
                />
                <label for="tile-break" class="tile-switch">{{ t('screenTools.break.cover') }}</label>
              </template>
            </HomeTile>
          </li>
        </ul>
      </section>

    </main>

    <!-- Each drawer is its own component; they share this page's notice, run and busy (homeDrawer.ts). -->
    <ConnectionsDrawer :overview="overview" :live="live" />
    <ChatDrawer v-model:log="chatLog" :title="moduleName('chat')" :broadcaster-id="broadcasterId" :disconnected="live.status === 'failed'" />
    <AlertsDrawer :title="moduleName('alerts')" />
    <RedemptionDrawer :title="moduleName('redemption')" :recent="recent" />
    <CheckinDrawer :title="moduleName('checkin')" :today="checkinToday" />
    <!-- 呈現佇列 opens a drawer like the other tiles; the queue's own heading gives way to the drawer's title. -->
    <AppDrawer id="queue" :title="moduleName('queue')">
      <template #now>
        <QueueArea :status="live.queue" in-drawer />
      </template>
    </AppDrawer>
    <SoundDrawer :title="moduleName('sound')" :sounds="sounds" />
    <SceneDrawer :title="moduleName('scene')" :live="live" />
    <MixerDrawer :title="moduleName('mixer')" :live="live" />
    <CounterDrawer :title="moduleName('counter')" :counters="live.counters" />
    <ScreenToolsDrawer :title="moduleName('screen-tools')" :live="live" />
    <RehearsalDrawer :title="moduleName('rehearsal')" />
    <ImageSizesDrawer :title="moduleName('image-sizes')" />
    <AppDrawer id="stream-info" :title="moduleName('stream-info')" full-page="/stream-info">
      <template #now>
        <p>{{ t('streamInfo.tile') }}</p>
      </template>
    </AppDrawer>
    <RewardsDrawer :title="moduleName('rewards')" />
    <AppDrawer id="polls" :title="moduleName('polls')" full-page="/polls">
      <template #now>
        <p>{{ t('polls.tile') }}</p>
      </template>
    </AppDrawer>
    <CommandsDrawer :title="moduleName('commands')" />
    <SongsDrawer :title="moduleName('songs')" :changed="songsChanged" />
    <BgmDrawer :title="moduleName('bgm')" :live="live" :playlists="playlists" />
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import ToggleSwitch from 'primevue/toggleswitch'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import QueueArea from './QueueArea.vue'
import StageCard from './StageCard.vue'
import { testSound } from './simulate'
import BlockClose from './BlockClose.vue'
import { STAGES, useStage } from './stage'
import { computed, nextTick, onBeforeUnmount, onMounted, provide, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import AppDrawer from './AppDrawer.vue'
import AlertsDrawer from './AlertsDrawer.vue'
import BgmDrawer from './BgmDrawer.vue'
import ChatDrawer from './ChatDrawer.vue'
import CheckinDrawer from './CheckinDrawer.vue'
import CommandsDrawer from './CommandsDrawer.vue'
import ConnectionsDrawer from './ConnectionsDrawer.vue'
import CounterDrawer from './CounterDrawer.vue'
import ImageSizesDrawer from './ImageSizesDrawer.vue'
import MixerDrawer from './MixerDrawer.vue'
import RedemptionDrawer from './RedemptionDrawer.vue'
import RehearsalDrawer from './RehearsalDrawer.vue'
import RewardsDrawer from './RewardsDrawer.vue'
import SceneDrawer from './SceneDrawer.vue'
import ScreenToolsDrawer from './ScreenToolsDrawer.vue'
import SongsDrawer from './SongsDrawer.vue'
import SoundDrawer from './SoundDrawer.vue'
import { HOME_DRAWER } from './homeDrawer'
import ArrangeTiles from './ArrangeTiles.vue'
import { EMPTY_LOG, applyChat } from './chatLog'
import { useDock } from './dock'
import { useDrawer } from './drawer'
import {
  type Line,
  COUNT,
  arrangedTiles,
  errorToast,
  successToast,
  EXPANDS,
  TILES,
  homeTiles,
  stageDefaults,
  type Stage,
  type Tile,
  useLiveEvents,
} from './home'
import { isMasked, reveal, saveAfterNumbers, useAfterNumbers } from './afterNumbers'
import HomeTile from './HomeTile.vue'
import { obsProblems, useObsSources } from './obsSources'
import { useCheckinOutbox } from './checkinOutbox'
import { changesCheckins } from './checkins'
import { focusReturnTarget, keepFocus } from './page'
import { mixSections, useMic } from './mixer'
import { obsReady } from './scenes'
import { stepCounter } from './counters'
import { hideBreak, showBreak } from './screenTools'
import { changesFeed, feedResult, feedText } from './feed'
import { changesRecords } from './redemptions'
import { useHomeData } from './homeData'
import { tileLines, tileName } from './homeLines'
import UpdateNotice from './UpdateNotice.vue'

const { t, te, locale } = useI18n()
const toast = useToast()
// Set once: the app shell renders the target before any page (absent when a test mounts home alone).
const skipLinks = !!document.getElementById('skip-links')

const overview = ref<Schemas['Overview']>()
const overviewFailed = ref(false)
const modules = ref<Schemas['ModuleState'][]>()
const loadFailed = ref(false)
const busy = ref(false)
const templateError = ref<string | null>(null)
const notice = ref<{ text: string; error: boolean }>()
const tilesEl = ref<HTMLElement>()
const topEl = ref<HTMLElement>()
const STAGE_ICONS: Record<Stage, string> = {
  before: 'pi pi-list-check',
  live: 'pi pi-circle-fill stage-live',
  after: 'pi pi-flag',
}
const setTopHeight = () => {
  const el = topEl.value
  const pinned = el && getComputedStyle(el).position === 'sticky'
  document.documentElement.style.setProperty('--page-top-h', `${pinned ? el.offsetHeight : 0}px`)
}
const topObserver = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(setTopHeight)
// Saved tile order (`null`: never arranged). Only this page changes it, so it is read once with the modules.
// Each stage's saved tile order (`null`: never arranged, its defaults).
const savedTiles = ref<Record<Stage, string[] | null>>({ before: null, live: null, after: null })
const arranging = ref(false)
const arrangeButton = ref<{ $el: HTMLElement }>()
const doneButton = ref<{ $el: HTMLElement }>()
let savingTiles = false
let timer: number | undefined

const drawer = useDrawer()
watch(drawer.current, () => (notice.value = undefined))
// Kept here, not in the drawer, so messages keep arriving while it is closed.
const chatLog = shallowRef(EMPTY_LOG)
const { recent, loadRecent, checkinToday, loadCheckinToday, sounds, loadSounds, playlists, feed, reloadFeedSoon } = useHomeData(
  drawer,
  (e) => drawerFailed(e),
)
const songsChanged = ref(0)
const live = useLiveEvents(2000, (msg) => {
  if (msg.type === 'songs.changed' || msg.type === 'snapshot') songsChanged.value++
  if (changesFeed(msg.type)) reloadFeedSoon()
  chatLog.value = applyChat(chatLog.value, msg)
  if (changesRecords(msg)) void loadRecent()
  if (changesCheckins(msg)) void loadCheckinToday()
})
const formatTime = (iso: string) => new Date(iso).toLocaleTimeString(locale.value, { timeStyle: 'short' })
const broadcasterId = computed(() =>
  overview.value?.status.state === 'signed_in' ? overview.value.status.user_id : undefined,
)
const dock = useDock()
const stage = useStage({ live: () => live.value, modules: () => modules.value })
const shownStage = stage.shown
// 待處理 shows only with items.
const hiddenNow = computed(() => {
  const hidden = new Set<string>()
  if (!pending.value.length) hidden.add('pending')
  // The docked chat panel already shows the chat.
  if (dock.docked) hidden.add('chat')
  return hidden
})
// Which of the small tiles are open into their block; this window only, never saved.
const opened = ref(new Set<string>())
const sizeOf = (tile: Tile) => (tile.id in EXPANDS ? (opened.value.has(tile.id) ? EXPANDS[tile.id] : undefined) : tile.size)
/** Opens a small tile into its block (or closes it) and keeps focus on the same place: the block's 收合, or the tile. */
async function setOpened(id: string, open: boolean) {
  const next = new Set(opened.value)
  if (open) next.add(id)
  else next.delete(id)
  opened.value = next
  await nextTick()
  tilesEl.value?.querySelector<HTMLElement>(`[data-expand="${id}"]`)?.focus()
}
const openBlock = (id: string) => setOpened(id, true)
const closeBlock = (id: string) => setOpened(id, false)
const tiles = computed(() =>
  homeTiles(
    modules.value ?? [],
    arrangedTiles(savedTiles.value[shownStage.value], TILES, stageDefaults(shownStage.value)).filter(
      (t) => !hiddenNow.value.has(t.id),
    ),
  ),
)
// OBS problems: read again whenever OBS's scenes or items change, so a fixed one goes away.
const obsSources = useObsSources()
watch(
  // Scenes, items and the scene on air: not a fader or mute moving.
  () => JSON.stringify([live.value.obsState?.scene, live.value.obsState?.scenes]),
  () => void useObsSources(true),
)
const sceneRules = shallowRef<Schemas['SceneRuleBody'][]>([])
// The modules are loaded before home opens, so this reads right away too; only the latest read's answer shows.
let rulesRead = 0
watch(
  () => modules.value?.some((m) => m.id === 'bgm' && m.enabled),
  async (bgm) => {
    const mine = ++rulesRead
    const rules = bgm ? await api(client.GET('/api/admin/music')).then((m) => m.scene_rules, () => []) : []
    if (mine === rulesRead) sceneRules.value = rules
  },
  { immediate: true },
)
// Check-in redemptions that kept failing, read without opening the drawer (the poll is shared with it).
const checkinOn = computed(() => !!modules.value?.some((m) => m.id === 'checkin' && m.enabled))
const { outbox } = useCheckinOutbox(() => checkinOn.value)
const pending = computed<{ id: string; text: string; to?: string; drawer?: string }[]>(() => {
  const off = new Set((modules.value ?? []).filter((m) => !m.enabled).map((m) => m.id))
  const dead = checkinOn.value ? (outbox.value?.dead_letters.length ?? 0) : 0
  return [
    ...(dead ? [{ id: 'checkin:dead', text: t('home.pendingCheckin', { n: dead }), drawer: 'checkin' }] : []),
    ...obsProblems(obsSources.value, sceneRules.value, off, t),
  ]
})

const lines = computed(() =>
  tileLines(
    { live: live.value, recent: recent.value, checkinToday: checkinToday.value, sounds: sounds.value, playlists: playlists.value, feed: feed.value, stats: statsLine(), mixer: mixerLine() },
    t,
  ),
)





// 下播後 numbers: masked ones show 「––」 (a tile's only when it has a number); 整頁不顯示 folds the tiles behind one
// card, and 「這次看一下」 opens them for this visit only.
type NumbersMode = Schemas['NumbersMode']
const NUMBER_MODES: NumbersMode[] = ['show', 'mask', 'hide']
const numbers = useAfterNumbers()
const peek = ref(false)
watch(shownStage, () => (peek.value = false))
const folded = computed(() => shownStage.value === 'after' && numbers.mode === 'hide' && !peek.value)
const maskedNumber = (id: string) => {
  if (shownStage.value !== 'after' || !isMasked(id)) return false
  const line = lines.value[id]
  // Only a count is a number to mask: a scene, a song or a state (開著, 顯示中) stays, and so does its tile's button.
  return id.startsWith('stats.') || (line?.state === 'ready' && !!line.num && COUNT.test(line.num))
}
const saveNumbers = (change: Partial<Schemas['AfterNumbers']>) => tileAction(saveAfterNumbers(change))
const revealNumber = (id: string) => tileAction(reveal(id))

// A tile's in-place action reports a failure as a toast: its drawer, where notices show, is closed.
const tileAction = (p: Promise<unknown>) =>
  p.catch((e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e))))
// The break card from a tile shows again the way it was last set (minutes or until a time).
const breakAgain = (card: Schemas['BreakPayload']) =>
  card.mode === 'until' ? { until: card.untilTime } : { minutes: card.minutes }

// 音訊混音: the chosen mic only (never guessed); the tile shows OBS's state, so a mute shows once OBS has made it.
const mic = useMic()
const micState = computed(() => {
  const obs = live.value.obsState
  if (!obsReady(obs)) return { reason: t('scene.notConnected') }
  if (mic.value === undefined) return {}
  if (!mic.value) return { reason: t('mixer.tile.noMic') }
  const input = obs.inputs.find((i) => i.name === mic.value)
  return input ? { input } : { reason: t('mixer.micMissing', { input: mic.value }) }
})
const micBusy = ref(false)
async function toggleMic() {
  const input = micState.value.input
  if (!input) return
  micBusy.value = true
  await tileAction(api(client.PUT('/api/admin/obs/input', { body: { input: input.name, muted: !input.muted } })))
  micBusy.value = false
}
const mixLive = computed(() => (obsReady(live.value.obsState) ? mixSections(live.value.obsState, live.value.obsState.scene).on : []))
function mixerLine(): Line {
  const { input, reason } = micState.value
  if (!input) return reason ? { state: 'ready', text: reason } : { state: 'loading' }
  return {
    state: 'ready',
    num: t(input.muted ? 'mixer.tile.muted' : 'mixer.tile.on'),
    unit: input.name,
    text: t('mixer.tile.inputs', { n: mixLive.value.length }),
  }
}

const drawerFailed = (e: unknown) =>
  (notice.value = { text: e instanceof ApiError ? errorMessage(e.code) : String(e), error: true })
const attention = computed<Record<string, boolean>>(() => ({
  chat: templateError.value !== null,
  mixer: !!micState.value.input?.muted,
}))

/** The small 本場直播 tile: how long it has run, with redemptions and subs. */
function statsLine(): Line {
  if (stage.failed.value) return { state: 'failed' }
  const s = stage.session.value
  if (!s) return { state: 'ready', text: t(stage.notLive.value ? 'stage.notLive' : 'stage.waiting') }
  // 下播後 with numbers masked: its counts too, each until revealed.
  const n = (key: string, v: number) => (shownStage.value === 'after' && isMasked(`stats.${key}`) ? '––' : v)
  return { state: 'ready', num: stage.when.value, text: t('home.statsLine', { redemptions: n('redemptions', s.redemptions), subs: n('subs', s.subs) }) }
}



const moduleName = (id: string) => tileName(id, t, te)

async function refreshOverview() {
  try {
    overview.value = await api(client.GET('/api/admin/twitch'))
    overviewFailed.value = false
  } catch (e) {
    overviewFailed.value = true
    throw e
  }
}

async function refresh() {
  await refreshOverview()
  if (!modules.value) {
    const [before, live, after] = await Promise.all(
      STAGES.map((stage) => api(client.GET('/api/admin/home-tiles', { params: { query: { stage } } }))),
    )
    savedTiles.value = { before, live, after }
  }
  modules.value = await api(client.GET('/api/admin/modules'))
  await refreshTemplate()
}

async function refreshTemplate() {
  templateError.value = (await api(client.GET('/api/admin/templates/chat'))).error
}

// Toasts sit under a modal drawer's top layer, so results show inside the drawer while it is modal.
function notify(text: string, error: boolean) {
  if (document.querySelector('.drawer-dialog[open]')) notice.value = { text, error }
  else toast.add(error ? errorToast(text) : successToast(text))
}

// Tiles and lights are disclosure buttons: a second press closes their drawer.
const toggle = (id: string, el: EventTarget | null) =>
  drawer.current.value === id ? drawer.close() : drawer.open(id, el)

async function run(action: () => Promise<unknown>) {
  busy.value = true
  notice.value = undefined
  try {
    await action()
    await refresh()
  } catch (e) {
    loadFailed.value = !modules.value
    notify(e instanceof ApiError ? errorMessage(e.code) : String(e), true)
  } finally {
    busy.value = false
  }
}

provide(HOME_DRAWER, { notice, busy, run, notify, failed: drawerFailed })


// The section itself shows no focus ring, so it only takes focus while it has nothing focusable (loading, failed).
const firstTile = () =>
  tilesEl.value?.querySelector<HTMLElement>('[data-drawer-tile], [data-expand], [data-drawer-arrange]') ?? tilesEl.value
const focusTiles = () => firstTile()?.focus()

/** Closes an open drawer first (submitting its changes); a failed submit keeps the drawer and the normal home. */
async function enterArrange() {
  if (!(await drawer.close())) return
  arranging.value = true
  await nextTick()
  doneButton.value?.$el.focus()
}

async function leaveArrange() {
  arranging.value = false
  await nextTick()
  arrangeButton.value?.$el.focus()
}

/**
 * Shows the tile order `next` at once and saves it; a failure puts the previous order back and reports it, even
 * after arrange mode was left. Returns nothing while another save runs, so saves never overlap.
 */
function saveTiles(next: string[] | null): Promise<boolean> | undefined {
  if (savingTiles) return
  savingTiles = true
  // The stage being edited now; a failure puts back that stage's order even if another is shown by then.
  const stage = shownStage.value
  const previous = savedTiles.value[stage]
  savedTiles.value = { ...savedTiles.value, [stage]: next }
  return api(client.PUT('/api/admin/home-tiles', { params: { query: { stage } }, body: next }))
    .then(
      () => true,
      (e) => {
        savedTiles.value = { ...savedTiles.value, [stage]: previous }
        notify(e instanceof ApiError ? errorMessage(e.code) : String(e), true)
        return false
      },
    )
    .finally(() => (savingTiles = false))
}

// Esc leaves arrange mode, but an open drawer or dialog (the close hint) takes it first. Listens in the capture
// phase: browsers run microtasks between listeners, so the drawer's own Esc listener may have finished closing it
// before a bubbling listener here would run.
function onKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape' || e.defaultPrevented || !arranging.value) return
  if (!drawer.current.value && !document.querySelector('dialog[open]')) leaveArrange()
}

onMounted(async () => {
  setTopHeight()
  if (topEl.value) topObserver?.observe(topEl.value)
  addEventListener('resize', setTopHeight)
  document.addEventListener('keydown', onKeydown, true)
  // Statuses and the template are polled, so a lost sign-in or a broken template edit shows up without a reload.
  // Until the first full load succeeds, retry all of it, so one startup failure does not leave the tiles empty.
  // Started before any await: leaving the page before the first load ends must still stop it.
  timer = window.setInterval(() => {
    const poll = modules.value ? Promise.all([refreshOverview(), refreshTemplate()]) : refresh()
    poll.catch(() => {})
  }, 2000)
  await run(async () => {})
  void loadSounds()
  await focusReturnTarget()
})
onBeforeUnmount(() => {
  topObserver?.disconnect()
  removeEventListener('resize', setTopHeight)
  document.documentElement.style.removeProperty('--page-top-h')
  window.clearInterval(timer)
  document.removeEventListener('keydown', onKeydown, true)
})
</script>

<style scoped>
.feed-list li {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem 0.75rem;
  padding: 0.25rem 0;
}

.feed-text {
  flex: 1 1 12rem;
  min-width: 0;
  overflow-wrap: anywhere;
}

.feed-result {
  color: var(--p-text-muted-color);
}

.feed-text .tag {
  margin-inline-start: 0.5rem;
  padding: 0 0.5rem;
  border-radius: 999px;
  font-size: 0.85em;
  background: var(--p-content-hover-background);
}

.folded {
  display: grid;
  justify-items: start;
  gap: 0.75rem;
}

.folded p {
  margin: 0;
}

.tile-switch {
  font-size: 0.8125rem;
  color: var(--p-text-muted-color);
}

.top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem 1rem;
  margin-bottom: 0.75rem;
}

/* Same sticky rule as FullPage.vue: pinned under the app bar while the window is wide and tall enough. */
@media (min-width: 601px) and (min-height: 560px) {
  .top {
    position: sticky;
    top: var(--appbar-h, 0px);
    z-index: 2;
    margin-top: -0.5rem;
    margin-bottom: 0;
    padding: 0.5rem 0 0.75rem;
    background: var(--p-surface-950);
  }
}

.stage-switch :deep(.p-togglebutton-content) {
  gap: 0.375rem;
}

.stage-switch :deep(.stage-live) {
  color: var(--p-red-400);
  font-size: 0.5rem;
}

.lights {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

/* Phones: the three stages stay on one row and scroll sideways when they do not fit. */
@media (max-width: 599.98px) {
  .stage-switch {
    max-width: 100%;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .stage-switch :deep(.p-selectbutton) {
    flex-wrap: nowrap;
  }

  .stage-switch :deep(.p-togglebutton) {
    white-space: nowrap;
  }
}

.plain {
  list-style: none;
  padding: 0;
}

.link-button {
  padding: 0;
  border: 0;
  background: none;
  color: var(--p-primary-color);
  font: inherit;
  text-align: start;
  text-decoration: underline;
  cursor: pointer;
}

.tile-reason {
  flex-basis: 100%;
  font-size: 0.8125rem;
  color: var(--p-text-muted-color);
}

.tag {
  font-size: 0.8125rem;
  color: var(--p-primary-color);
}

.grid {
  display: grid;
  /* 12.5rem = 200px at the default font size. */
  grid-template-columns: repeat(auto-fill, minmax(min(12.5rem, 100%), 1fr));
  gap: 0.75rem;
}

/* The prototype's 12-column home: a tile takes 3 columns, `size-m` 4, `size-w` the whole row. */
.tiles {
  display: grid;
  grid-template-columns: repeat(12, minmax(0, 1fr));
  gap: 0.75rem;
  align-items: start;
}

.tiles > li {
  grid-column: span 3;
  min-width: 0;
}

.tiles > li.size-m {
  grid-column: span 4;
}

.tiles > li.size-l {
  grid-column: span 8;
}

.tiles > li.size-w {
  grid-column: 1 / -1;
}

/* Three small tiles a row, then two: by the home's own width, so an open drawer or chat panel reflows them too. */
@container view (max-width: 60rem) {
  .tiles > li {
    grid-column: span 4;
  }

  .tiles > li.size-m {
    grid-column: span 6;
  }

  .tiles > li.size-l {
    grid-column: 1 / -1;
  }
}

@container view (max-width: 40rem) {
  .tiles > li {
    grid-column: span 6;
  }

  .tiles > li.size-m {
    grid-column: 1 / -1;
  }
}

.block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
}

/* QueueArea's root takes this page's scoped styles, so a queue block on the grid would get the same card. */
.block,
.tiles .queue {
  padding: 1rem 1.125rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-900);
}

.block h2 {
  margin: 0;
}

@container view (max-width: 36rem) {
  .grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

#tiles:focus {
  outline: none;
}

/* Wider than 1000px the drawer is an aside beside the page. */
.layout {
  display: flex;
  align-items: flex-start;
}

.layout > main {
  flex: 1;
  min-width: 0;
}

.stage-switch .manual {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
