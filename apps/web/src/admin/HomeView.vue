<template>
  <Teleport to="#skip-links" :disabled="!skipLinks">
    <a href="#tiles" class="skip-link" @click.prevent="focusTiles">{{ t('home.skip') }}</a>
  </Teleport>
  <div class="layout">
    <main>
      <header class="top">
        <!-- The app bar shows the name; the status lights and settings live there too. -->
        <h1 class="sr-only">{{ t('app.name') }}</h1>
        <div class="lights">
          <Button v-if="arranging" ref="doneButton" :label="t('arrange.done')" icon="pi pi-check" @click="leaveArrange" />
          <Button
            v-else
            ref="arrangeButton"
            icon="pi pi-pencil"
            severity="secondary"
            text
            :aria-label="t('home.arrange')"
            :disabled="!modules"
            @click="enterArrange"
          />
        </div>
      </header>
      <p v-if="templateError" role="alert">{{ errorMessage(templateError) }}</p>
      <UpdateNotice @dismissed="keepFocus(firstTile)" />

      <!-- Later slices add pending items; the list shows only when it has some. -->
      <section v-if="pending.length" aria-labelledby="pending-heading">
        <h2 id="pending-heading">{{ t('home.pending') }}</h2>
        <ul class="plain">
          <li v-for="p in pending" :key="p.id">
            <Button :label="p.text" text @click="toggle(p.panel, $event.currentTarget)" />
          </li>
        </ul>
      </section>

      <StageCard v-if="!arranging" :live="live" :modules="modules" />
      <QueueArea v-if="!arranging" :status="live.queue" />

      <section id="tiles" ref="tilesEl" tabindex="-1" :aria-label="t('home.tiles')">
        <p v-if="!modules">{{ loadFailed ? t('home.loadFailed') : t('app.loading') }}</p>
        <ArrangeTiles
          v-else-if="arranging"
          :saved="savedTiles"
          :save="saveTiles"
          :modules="modules"
          :name="moduleName"
        />
        <div v-else-if="!groups.length" class="stack">
          <p>{{ t('home.empty') }}</p>
          <Button :label="t('home.arrange')" icon="pi pi-pencil" outlined data-drawer-arrange @click="enterArrange" />
        </div>
        <template v-for="g in groups" v-else :key="g.group">
          <h2 :id="`group-${g.group}`">{{ t(`home.groups.${g.group}`) }}</h2>
          <ul class="plain grid" :aria-labelledby="`group-${g.group}`">
            <li v-for="tile in g.tiles" :key="tile.id">
              <HomeTile
                :id="tile.id"
                :name="moduleName(tile.id)"
                :icon="tile.icon"
                :line="lines[tile.id] ?? { state: 'loading' }"
                :attention="attention[tile.id]"
                :expanded="drawer.current.value === tile.id"
                @click="toggle(tile.id, $event.currentTarget)"
              />
            </li>
          </ul>
        </template>
      </section>

      <!-- Features of the design not built yet: each tile opens its coming-soon page (no drawer). -->
      <section v-if="!arranging" aria-labelledby="soon-heading">
        <h2 id="soon-heading">{{ t('home.soon') }}</h2>
        <ul class="plain grid">
          <li v-for="p in soonTiles" :key="p.path">
            <RouterLink :to="p.path" class="soon-tile" :data-page-opener="`soon-${p.soon!.id}`">
              <i :class="p.soon!.icon" aria-hidden="true"></i>
              <span class="name">{{ t(p.label) }}</span>
              <span class="line">{{ t(`pages.${p.soon!.id}.desc`) }}</span>
              <span class="tag">{{ t('home.soon') }}</span>
            </RouterLink>
          </li>
        </ul>
      </section>
    </main>

    <AppDrawer id="connections" :title="t('home.connections')">
      <template #now>
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
        <h4>{{ t('twitch.heading') }}</h4>
        <TwitchSignIn :overview="overview" :busy="busy" :run="run" />

        <h4>OBS</h4>
        <p>
          {{ t(obs.key) }}
          <template v-if="live.obs?.reason">— {{ errorMessage(live.obs.reason) }}</template>
        </p>
      </template>
    </AppDrawer>

    <AppDrawer id="chat" :title="moduleName('chat')" full-page="/chat">
      <template #actions>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('chat')" />
        <form @submit.prevent="simulateChat">
          <p>{{ t('simulate.hint') }}</p>
          <label for="sim-user">{{ t('simulate.userName') }}</label>
          <InputText id="sim-user" v-model="simUser" autocomplete="off" maxlength="25" />
          <label for="sim-text">{{ t('simulate.text') }}</label>
          <InputText id="sim-text" v-model="simText" autocomplete="off" maxlength="500" required />
          <Button type="submit" :label="t('simulate.send')" icon="pi pi-send" :disabled="busy" />
        </form>
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
      <template #now>
        <ChatNow v-model:log="chatLog" :broadcaster-id="broadcasterId" :disconnected="live.status === 'failed'" />
      </template>
    </AppDrawer>

    <AppDrawer id="alerts" :title="moduleName('alerts')" full-page="/alerts">
      <template #actions>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('alerts')" />
        <form @submit.prevent="simulateAlert">
          <p>{{ t('simulate.hint') }}</p>
          <span id="sim-kind">{{ t('simulate.kind') }}</span>
          <SelectButton
            v-model="simAlert.kind"
            :options="ALERT_KINDS"
            :option-label="(k: AlertKind) => t(`alerts.kinds.${k}`)"
            :allow-empty="false"
            aria-labelledby="sim-kind"
          />
          <label for="sim-alert-user">{{ t('simulate.userName') }}</label>
          <InputText id="sim-alert-user" v-model="simAlert.userName" autocomplete="off" maxlength="25" />
          <template v-if="simAlert.kind in AMOUNT_MAX">
            <label for="sim-amount">{{ t(`alerts.amount.${simAlert.kind}`) }}</label>
            <InputNumber
              v-model="simAlert.amount"
              input-id="sim-amount"
              :min="1"
              :max="AMOUNT_MAX[simAlert.kind]"
              :use-grouping="false"
              required
            />
          </template>
          <template v-if="simAlert.kind === 'resubscribe' || simAlert.kind === 'cheer'">
            <label for="sim-alert-message">{{ t('simulate.message') }}</label>
            <InputText id="sim-alert-message" v-model="simAlert.message" autocomplete="off" maxlength="500" />
          </template>
          <Button type="submit" :label="t('simulate.sendAlert')" icon="pi pi-send" :disabled="busy" />
        </form>
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
    </AppDrawer>

    <AppDrawer id="redemption" :title="moduleName('redemption')" full-page="/redemptions">
      <template #now>
        <p v-if="recent.state === 'failed'" role="alert">{{ t('redemption.loadFailed') }}</p>
        <p v-else-if="recent.state === 'loading'">{{ t('app.loading') }}</p>
        <RedemptionList v-else-if="recent.state === 'ready'" :records="recent.items" :empty="t('redemption.empty')" />
      </template>
    </AppDrawer>

    <AppDrawer id="checkin" :title="moduleName('checkin')" full-page="/checkin">
      <template #actions>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('checkin')" />
        <form @submit.prevent="simulateCheckin">
          <p>{{ t('simulate.hint') }}</p>
          <label for="sim-checkin-user">{{ t('simulate.userName') }}</label>
          <InputText id="sim-checkin-user" v-model="simCheckin.userName" autocomplete="off" maxlength="25" />
          <label for="sim-checkin-stamps">{{ t('simulate.stamps') }}</label>
          <InputNumber
            v-model="simCheckin.stamps"
            input-id="sim-checkin-stamps"
            :min="1"
            :max="100"
            :use-grouping="false"
            required
          />
          <Button type="submit" :label="t('simulate.sendCheckin')" icon="pi pi-send" :disabled="busy" />
        </form>
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
      <template #now>
        <p v-if="checkinToday.state === 'failed'" role="alert">{{ t('checkin.page.loadFailed') }}</p>
        <p v-else-if="checkinToday.state === 'loading'">{{ t('app.loading') }}</p>
        <template v-else-if="checkinToday.state === 'ready'">
          <p>{{ t('checkin.today', { n: checkinToday.today.count }) }}</p>
          <ul v-if="checkinToday.today.recent.length" class="recent-checkins">
            <li v-for="r in checkinToday.today.recent" :key="r.viewer_id">
              <time :datetime="r.occurred_at">{{ formatTime(r.occurred_at) }}</time> {{ r.user_name }}
            </li>
          </ul>
        </template>
      </template>
    </AppDrawer>

    <AppDrawer id="feed" :title="moduleName('feed')">
      <template #now>
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
      </template>
    </AppDrawer>

    <AppDrawer id="sound" :title="moduleName('sound')" full-page="/sounds">
      <template #actions>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('sound')" />
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
      <template #now>
        <p v-if="sounds.state === 'failed'" role="alert">{{ t('sound.loadFailed') }}</p>
        <p v-else-if="sounds.state === 'loading'">{{ t('app.loading') }}</p>
        <template v-else-if="sounds.state === 'ready'">
          <p v-if="!sounds.items.length">{{ t('sound.empty') }}</p>
          <ul v-else class="plain sound-list">
            <li v-for="s in sounds.items" :key="s.id">{{ s.label }}</li>
          </ul>
        </template>
      </template>
    </AppDrawer>

    <AppDrawer id="scene" :title="moduleName('scene')" full-page="/scenes">
      <template #actions>
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
      <template #now>
        <SceneList v-if="obsReady(live.obsState)" :state="live.obsState" @error="drawerFailed" />
        <p v-else>{{ t('scene.notConnected') }}</p>
      </template>
    </AppDrawer>

    <AppDrawer id="counter" :title="moduleName('counter')" full-page="/counters">
      <template #actions>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('counter')" />
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
      <template #now>
        <p v-if="!live.counters">{{ t('app.loading') }}</p>
        <p v-else-if="!live.counters.length">{{ t('counter.empty') }}</p>
        <ul v-else class="plain counter-list">
          <li v-for="c in live.counters" :key="c.id">
            <span>{{ c.name }}</span>
            <output class="counter-value">{{ c.value }}</output>
            <Button
              icon="pi pi-plus"
              :aria-label="`${t('counter.plusOne')}：${c.name}`"
              outlined
              @click="stepCounter(c.id, 1).catch(drawerFailed)"
            />
          </li>
        </ul>
      </template>
    </AppDrawer>

    <AppDrawer id="bgm" :title="moduleName('bgm')" full-page="/music">
      <template #actions>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('bgm')" />
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
      <template #now>
        <MusicPlayer :event="live.bgm" :playlists="playlists" @error="drawerFailed" />
      </template>
    </AppDrawer>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import QueueArea from './QueueArea.vue'
import StageCard from './StageCard.vue'
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ALERT_KINDS, type AlertKind, alertTitle } from '../overlay/alerts/alerts'
import { ApiError, type Schemas, api, client } from './api'
import AppDrawer from './AppDrawer.vue'
import ArrangeTiles from './ArrangeTiles.vue'
import ChatNow from './ChatNow.vue'
import { EMPTY_LOG, applyChat } from './chatLog'
import { useDrawer } from './drawer'
import {
  type Line,
  arrangedTiles,
  errorToast,
  obsLight,
  successToast,
  tileGroups,
  useLiveEvents,
} from './home'
import HomeTile from './HomeTile.vue'
import { changesCheckins } from './checkins'
import { SOON_PAGES } from './tabs'
import { focusReturnTarget, keepFocus } from './page'
import RedemptionList from './RedemptionList.vue'
import SceneList from './SceneList.vue'
import { obsReady } from './scenes'
import { counterLine, stepCounter } from './counters'
import { type FeedItem, changesFeed, feedLine, feedResult, feedText } from './feed'
import MusicPlayer from './MusicPlayer.vue'
import { playerLine } from './music'
import { RECENT, type RedemptionRecord, changesRecords, noFilters, redemptionQuery } from './redemptions'
import TwitchSignIn from './TwitchSignIn.vue'
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
const simUser = ref('')
const simText = ref('')
// The Host's limits for the kinds that take an amount (crates/host/src/admin.rs).
const AMOUNT_MAX: Partial<Record<AlertKind, number>> = { resubscribe: 100000, gift: 100, cheer: 100000, raid: 100000 }
const simCheckin = reactive({ userName: '', stamps: 3 as number | null })
const simAlert = reactive({ kind: 'follow' as AlertKind, userName: '', amount: 5 as number | null, message: '' })
const templateError = ref<string | null>(null)
const notice = ref<{ text: string; error: boolean }>()
const tilesEl = ref<HTMLElement>()
// Saved tile order (`null`: never arranged). Only this page changes it, so it is read once with the modules.
const savedTiles = ref<string[] | null>(null)
const arranging = ref(false)
const arrangeButton = ref<{ $el: HTMLElement }>()
const doneButton = ref<{ $el: HTMLElement }>()
let savingTiles = false
let timer: number | undefined

const drawer = useDrawer()
watch(drawer.current, () => (notice.value = undefined))
// Kept here, not in the drawer, so messages keep arriving while it is closed.
const chatLog = shallowRef(EMPTY_LOG)
// The latest records, for the redemption tile and drawer; reloaded on every change and reconnect.
const recent = shallowRef<{ state: 'loading' | 'failed' } | { state: 'ready'; items: RedemptionRecord[] }>({
  state: 'loading',
})
let recentRequest = 0
async function loadRecent() {
  const mine = ++recentRequest
  try {
    const page = await api(client.GET('/api/admin/redemptions', { params: { query: redemptionQuery(noFilters(), 0, RECENT) } }))
    if (mine === recentRequest) recent.value = { state: 'ready', items: page.items }
  } catch {
    if (mine === recentRequest) recent.value = { state: 'failed' }
  }
}
// Today's check-ins, for the check-in tile and drawer; reloaded on every check-in and reconnect.
const checkinToday = shallowRef<{ state: 'loading' | 'failed' } | { state: 'ready'; today: Schemas['CheckinToday'] }>({
  state: 'loading',
})
let checkinRequest = 0
async function loadCheckinToday() {
  const mine = ++checkinRequest
  try {
    const today = await api(client.GET('/api/admin/checkin/today'))
    if (mine === checkinRequest) checkinToday.value = { state: 'ready', today }
  } catch {
    if (mine === checkinRequest) checkinToday.value = { state: 'failed' }
  }
}
// The sound library, for the sound tile and drawer; reloaded when the drawer opens.
const sounds = shallowRef<{ state: 'loading' | 'failed' } | { state: 'ready'; items: Schemas['Sound'][] }>({
  state: 'loading',
})
async function loadSounds() {
  try {
    sounds.value = { state: 'ready', items: (await api(client.GET('/api/admin/sounds'))).sounds }
  } catch {
    sounds.value = { state: 'failed' }
  }
}
watch(drawer.current, (id) => id === 'sound' && void loadSounds())
// The playlists for the music drawer; reloaded when it opens.
const playlists = shallowRef<Schemas['MusicPlaylist'][]>([])
async function loadPlaylists() {
  try {
    playlists.value = (await api(client.GET('/api/admin/music'))).playlists
  } catch (e) {
    drawerFailed(e)
  }
}
watch(drawer.current, (id) => id === 'bgm' && void loadPlaylists())
// This stream's alerts, redemptions and check-ins with what became of them; reloaded (at most once a second) when
// a live message may have changed them.
const feed = shallowRef<{ state: 'loading' | 'failed' } | { state: 'ready'; items: FeedItem[] }>({ state: 'loading' })
let feedTimer: ReturnType<typeof setTimeout> | undefined
async function loadFeed() {
  feedTimer = undefined
  try {
    feed.value = { state: 'ready', items: await api(client.GET('/api/admin/feed')) }
  } catch {
    feed.value = { state: 'failed' }
  }
}
const reloadFeedSoon = () => (feedTimer ??= setTimeout(() => void loadFeed(), 1000))
onBeforeUnmount(() => clearTimeout(feedTimer))
const live = useLiveEvents(2000, (msg) => {
  if (changesFeed(msg.type)) reloadFeedSoon()
  chatLog.value = applyChat(chatLog.value, msg)
  if (changesRecords(msg)) void loadRecent()
  if (changesCheckins(msg)) void loadCheckinToday()
})
const formatTime = (iso: string) => new Date(iso).toLocaleTimeString(locale.value, { timeStyle: 'short' })
const broadcasterId = computed(() =>
  overview.value?.status.state === 'signed_in' ? overview.value.status.user_id : undefined,
)
const obs = computed(() => obsLight(live.value))
const groups = computed(() => tileGroups(modules.value ?? [], arrangedTiles(savedTiles.value)))
const soonTiles = SOON_PAGES.filter((p) => p.soon!.tile)
const pending = computed<{ id: string; text: string; panel: string }[]>(() => [])

const lines = computed<Record<string, Line>>(() => {
  const { status, lastChat, lastAlert } = live.value
  return {
    chat:
      status === 'open'
        ? { state: 'ready', text: lastChat ? `${lastChat.payload.userName}：${lastChat.payload.text}` : t('home.chatEmpty') }
        : { state: status },
    alerts:
      status === 'open'
        ? { state: 'ready', text: lastAlert ? alertTitle(lastAlert.payload) : t('alerts.empty') }
        : { state: status },
    redemption: recentLine(recent.value),
    checkin:
      checkinToday.value.state === 'ready'
        ? { state: 'ready', text: t('checkin.today', { n: checkinToday.value.today.count }) }
        : { state: checkinToday.value.state },
    sound:
      sounds.value.state === 'ready'
        ? {
            state: 'ready',
            text: sounds.value.items.length ? t('sound.count', { n: sounds.value.items.length }) : t('sound.empty'),
          }
        : { state: sounds.value.state },
    scene:
      status === 'open'
        ? { state: 'ready', text: live.value.obsState?.scene ?? t('scene.notConnected') }
        : { state: status },
    bgm: status !== 'open' ? { state: status } : live.value.bgm ? { state: 'ready', text: playerLine(live.value.bgm.payload) } : { state: 'loading' },
    feed: feed.value.state === 'ready' ? { state: 'ready', text: feedLine(feed.value.items) } : { state: feed.value.state },
    counter:
      status !== 'open'
        ? { state: status }
        : live.value.counters
          ? { state: 'ready', text: counterLine(live.value.counters) }
          : { state: 'loading' },
  }
})
const drawerFailed = (e: unknown) =>
  (notice.value = { text: e instanceof ApiError ? errorMessage(e.code) : String(e), error: true })
const attention = computed<Record<string, boolean>>(() => ({ chat: templateError.value !== null }))

function recentLine(r: typeof recent.value): Line {
  if (r.state !== 'ready') return { state: r.state }
  const last = r.items[0]
  return { state: 'ready', text: last ? `${last.user_name}：${last.reward_title}` : t('redemption.empty') }
}

const moduleName = (id: string) => (te(`modules.names.${id}`) ? t(`modules.names.${id}`) : id)

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
  if (!modules.value) savedTiles.value = await api(client.GET('/api/admin/home-tiles'))
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

const simulateChat = () =>
  run(async () => {
    await api(client.POST('/api/admin/simulate/chat', { body: { userName: simUser.value, text: simText.value } }))
    simText.value = ''
    notify(t('simulate.sent'), false)
  })

const simulateCheckin = () =>
  run(async () => {
    const { userName, stamps } = simCheckin
    await api(client.POST('/api/admin/simulate/checkin', { body: { userName, stamps: stamps ?? 0 } }))
    notify(t('simulate.checkinSent'), false)
  })

const simulateAlert = () =>
  run(async () => {
    const { kind, userName, amount, message } = simAlert
    await api(client.POST('/api/admin/simulate/alert', { body: { kind, userName, amount, message } }))
    notify(t('simulate.alertSent'), false)
  })

async function copyObsUrl(overlay: 'chat' | 'alerts' | 'checkin' | 'sound' | 'bgm' | 'counter') {
  notice.value = undefined
  try {
    // The admin window loads the Host URL, so its origin is what OBS needs.
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/${overlay}/`)
    notify(t('chat.copied'), false)
  } catch {
    notify(t('chat.copyFailed'), true)
  }
}

// The section itself shows no focus ring, so it only takes focus while it has nothing focusable (loading, failed).
const firstTile = () =>
  tilesEl.value?.querySelector<HTMLElement>('[data-drawer-tile], [data-drawer-arrange]') ?? tilesEl.value
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
  const previous = savedTiles.value
  savedTiles.value = next
  return api(client.PUT('/api/admin/home-tiles', { body: next }))
    .then(
      () => true,
      (e) => {
        savedTiles.value = previous
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

.counter-list li {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.counter-value {
  margin-inline-start: auto;
  font-size: 1.25rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.sound-list {
  margin: 0;
  overflow-wrap: anywhere;
}

.recent-checkins {
  list-style: none;
  padding: 0;
  margin: 0;
}

.top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 0.5rem 1rem;
}

.lights {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.plain {
  list-style: none;
  padding: 0;
}

.grid {
  display: grid;
  /* 12.5rem = 200px at the default font size. */
  grid-template-columns: repeat(auto-fill, minmax(min(12.5rem, 100%), 1fr));
  gap: 0.75rem;
}

@container view (max-width: 36rem) {
  .grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

.soon-tile {
  position: relative;
  display: grid;
  gap: 0.25rem;
  min-width: 0;
  padding: 0.75rem;
  border: 1px dashed var(--p-surface-500);
  border-radius: 6px;
  color: inherit;
  text-decoration: none;
}

.soon-tile:hover {
  border-color: var(--p-surface-400);
}

.soon-tile i {
  font-size: 1.25rem;
  color: var(--p-text-muted-color);
}

.soon-tile .name {
  font-weight: 600;
  overflow-wrap: anywhere;
}

/* One line; long values end in an ellipsis on purpose, like the feature tiles. */
.soon-tile .line {
  overflow: hidden;
  color: var(--p-text-muted-color);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.soon-tile .tag {
  position: absolute;
  top: 0.75rem;
  right: 0.75rem;
  padding: 0 0.5rem;
  border-radius: 999px;
  background: var(--p-surface-800);
  color: var(--p-text-muted-color);
  font-size: 0.75rem;
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

h4 {
  margin: 0;
}
</style>
