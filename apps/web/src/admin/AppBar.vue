<template>
  <!-- Sticky on every page (docs/decisions.md 管理 UI 版面); its height is --appbar-h for other sticky parts. -->
  <header ref="bar" class="app-bar" :class="{ scrolled }">
    <div class="bar-top">
      <span class="brand"><i class="pi pi-video" aria-hidden="true"></i>{{ t('app.name') }}</span>
      <div class="bar-status">
        <!-- What OBS is sending out right now; nothing while it is idle or not connected. -->
        <!-- Twitch's view: live since when and for how many viewers, or ended; nothing until Twitch has answered. -->
        <span v-if="live.stream?.state === 'live'" class="live-pill">
          <span class="dot" aria-hidden="true"></span>{{ t('nav.live') }}
          <span class="live-meta">{{ liveMeta }}</span>
        </span>
        <span v-else-if="live.stream?.state === 'offline'" class="live-pill offline">
          <span class="dot" aria-hidden="true"></span>{{ t('nav.offline') }}
        </span>
        <!-- Screen readers hear going live or ending, never the ticking timer. -->
        <span class="sr-only" aria-live="polite">{{ liveState }}</span>
        <span v-if="live.obsState?.streaming" class="on-air">{{ t('nav.streaming') }}</span>
        <span v-if="live.obsState?.recording" class="on-air">{{ t('nav.recording') }}</span>
        <!-- Narrower bars show only lights with a problem (one "connected" light when there is none); phones show a
             warning button instead, and only while a connection has a problem. -->
        <!-- A service the broadcaster does not use (設定 › 功能) has no light and is never a problem. -->
        <StatusLight
          v-if="!unused.includes('twitch')"
          name="Twitch"
          :light="twitch"
          :class="{ healthy: twitch.state === 'connected' }"
          v-bind="lightAttrs"
          @click="openConnections('twitch', $event.currentTarget)"
        />
        <StatusLight
          v-if="!unused.includes('obs')"
          name="OBS"
          :light="obs"
          :class="{ healthy: obs.state === 'connected' }"
          v-bind="lightAttrs"
          @click="openConnections('obs', $event.currentTarget)"
        />
        <StatusLight
          v-if="summary"
          class="all-healthy"
          :name="t('nav.connections')"
          :light="summary.light"
          v-bind="lightAttrs"
          @click="openConnections(summary.service, $event.currentTarget)"
        />
        <Button
          v-if="problems.length"
          class="problem"
          icon="pi pi-exclamation-triangle"
          text
          rounded
          :aria-label="t('nav.problem', { names: problems.map((p) => p.name).join('、') })"
          v-bind="lightAttrs"
          @click="openConnections(problems[0].section, $event.currentTarget)"
        />
        <Button
          ref="simulateButton"
          class="simulate-open"
          icon="pi pi-wrench"
          :label="t('simulate.dialog.title')"
          text
          :aria-label="t('simulate.dialog.title')"
          @click="simulate?.open()"
        />
        <Button
          v-if="!hidden.has('/chat')"
          icon="pi pi-comments"
          text
          rounded
          :aria-label="t('dock.title')"
          aria-controls="chat-dock"
          :aria-expanded="chatOpen"
          @click="toggleChat"
        />
        <Button
          class="estop"
          icon="pi pi-stop-circle"
          text
          rounded
          :aria-label="t('queue.stop')"
          :disabled="stopping"
          @click="stop"
        />
        <Button
          icon="pi pi-cog"
          text
          rounded
          :aria-label="t('settings.title')"
          :aria-current="route.path.startsWith('/settings') ? 'page' : undefined"
          data-page-opener="settings"
          @click="router.push('/settings')"
        />
      </div>
    </div>
    <Teleport to="#bottom-tabs" defer :disabled="tabBar.position !== 'bottom'">
    <nav ref="tabsEl" class="tabs" :class="{ bottom: tabBar.position === 'bottom' }" :aria-label="t('nav.label')">
      <template v-for="(g, i) in pinned" :key="g.id">
        <a
          v-if="groupUsable(g, hidden)"
          :href="router.resolve(target(g)).href"
          :hidden="i >= shown"
          :aria-current="groupCurrent(g)"
          data-group-tab
          @click.prevent="router.push(target(g))"
        >
          <i :class="g.icon" aria-hidden="true"></i>{{ t(`nav.groups.${g.id}`) }}
        </a>
        <!-- No built page to open (none built yet, or every built one turned off): grey, not focusable. -->
        <span v-else class="off" :hidden="i >= shown" aria-disabled="true" :title="t('home.unbuilt')" data-group-tab>
          <i :class="g.icon" aria-hidden="true"></i>{{ t(`nav.groups.${g.id}`) }}
          <span class="sr-only">{{ t('home.unbuilt') }}</span>
        </span>
      </template>
      <button
        ref="moreEl"
        type="button"
        class="more"
        :class="{ current: currentInMore }"
        :hidden="!measuring && !needMore"
        aria-haspopup="true"
        aria-controls="tabs-more"
        :aria-expanded="moreOpen"
        @click="moreMenu?.toggle($event)"
      >
        <i class="pi pi-ellipsis-h" aria-hidden="true"></i>{{ t('nav.more') }}
      </button>
      <!-- With nothing in "More", 「編輯分頁」 sits in the bar itself; otherwise it is the last item of "More". -->
      <button ref="editEl" :hidden="!measuring && needMore" type="button" class="more" @click="editor?.open()">
        <i class="pi pi-pencil" aria-hidden="true"></i>{{ t('nav.edit') }}
      </button>
      <Menu id="tabs-more" ref="moreMenu" :model="moreItems" :pt="morePt" popup @show="moreOpen = true" @hide="onMoreHide" />
    </nav>
    </Teleport>
    <TabBarEditor ref="editor" :groups="groups.map((g) => g.id)" />
    <SimulateDialog ref="simulate" :bgm="live.bgm" @closed="simulateButton?.$el.focus()" />
    <!-- Second row: the group's categories, or its pages when it has none. With the tabs at the bottom it goes
         there too, right above them. -->
    <Teleport to="#bottom-tabs" defer :disabled="tabBar.position !== 'bottom'">
    <nav
      v-if="subItems.length > 1"
      ref="subTabsEl"
      class="sub-tabs"
      :class="{ 'at-end': subAtEnd, bottom: tabBar.position === 'bottom' }"
      :aria-label="t('nav.sub')"
      @scroll.passive="checkSubEnd"
    >
      <template v-for="item in subItems" :key="item.key">
        <span v-if="!item.to" class="off" aria-disabled="true" :title="t('home.unbuilt')">
          {{ item.label }}<span class="sr-only">{{ t('home.unbuilt') }}</span>
        </span>
        <RouterLink v-else v-slot="{ href, navigate }" :to="item.to" custom>
          <a :href="href" :aria-current="item.current" @click="navOpened.path = item.to!; navigate($event)">{{ item.label }}</a>
        </RouterLink>
      </template>
    </nav>
    </Teleport>
  </header>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Menu from 'primevue/menu'
import type { MenuItem } from 'primevue/menuitem'
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch, watchEffect } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useToast } from 'primevue/usetoast'
import { errorMessage } from '../i18n'
import { ApiError, api, client } from './api'
import { drawerId, useDrawer } from './drawer'
import { clockText, errorToast, isDown, obsLight, servicesDown, twitchLight, twitchOverview, useLiveEvents } from './home'
import StatusLight from './StatusLight.vue'
import { summaryLight, useServices } from './services'
import { DOCK_WIDE_QUERY, saveDock, useDock } from './dock'
import { useModules } from './features'
import { useTabBar } from './tabbar'
import TabBarEditor from './TabBarEditor.vue'
import SimulateDialog from './SimulateDialog.vue'
import { navOpened } from './page'
import { GROUPS, type TabGroup, categoryOf, categoryTarget, firstPage, fittingTabs, groupOf, groupUsable, isGrey } from './tabs'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const drawer = useDrawer()
const toast = useToast()

// shortcut: home polls the same Twitch status and opens its own /ws/overlay for its tiles and drawers; share one
// source when more pages need these values.
const overview = twitchOverview
const overviewFailed = ref(false)
const live = useLiveEvents()

// How long the stream has run, ticking each second, and the viewers from the last minute's check.
const now = ref(Date.now())
const clock = window.setInterval(() => (now.value = Date.now()), 1000)
const liveState = computed(() => {
  const state = live.value.stream?.state
  return state === 'live' ? t('nav.live') : state === 'offline' ? t('nav.offline') : ''
})
const liveMeta = computed(() => {
  const s = live.value.stream
  if (s?.state !== 'live' || !s.startedAt) return ''
  const hms = clockText(now.value - Date.parse(s.startedAt))
  return s.viewers === undefined ? hms : `${hms} · ${t('nav.viewers', { n: s.viewers })}`
})
const twitch = computed(() => twitchLight(overview.value, overviewFailed.value))
const obs = computed(() => obsLight(live.value))
const summary = computed(() => summaryLight(unused.value, twitch.value, obs.value))
const services = useServices()
const unused = computed(() => services.unused)
// Pages and the feature list warn from these (servicesDown, home.ts).
watchEffect(() => {
  servicesDown.twitch = !unused.value.includes('twitch') && isDown(twitch.value)
  servicesDown.obs = !unused.value.includes('obs') && isDown(obs.value)
})
const problems = computed(() =>
  [
    { name: 'Twitch', section: 'twitch' as const, light: twitch.value },
    { name: 'OBS', section: 'obs' as const, light: obs.value },
  ].filter((c) => !unused.value.includes(c.section) && (c.light.state === 'error' || c.light.state === 'reconnecting')),
)

async function pollTwitch() {
  try {
    overview.value = await api(client.GET('/api/admin/twitch'))
    overviewFailed.value = false
  } catch {
    overviewFailed.value = true
  }
}

// On home the lights open the connections drawer; elsewhere they go to that section of settings.
const lightAttrs = computed(() =>
  route.path === '/'
    ? { 'aria-expanded': drawer.current.value === 'connections', 'aria-controls': drawerId('connections') }
    : {},
)
function openConnections(section: 'twitch' | 'obs', el: EventTarget | null) {
  if (route.path !== '/') void router.push({ path: '/settings/connections', hash: `#${section}` })
  else if (drawer.current.value === 'connections') void drawer.close()
  else void drawer.open('connections', el)
}

// A group tab goes back to the page last seen in that group, and a category to the one last seen in it.
const lastPage = reactive<Record<string, string>>({})
const lastInCategory = reactive<Record<string, string>>({})
const group = computed(() => groupOf(route.path))
watch(
  () => route.path,
  (path) => {
    const g = groupOf(path)
    if (g) lastPage[g.id] = path
    const c = categoryOf(path)
    if (c) lastInCategory[c.id] = path
  },
  { immediate: true },
)
// Pages of features turned off in settings are hidden, and so is a group left with none.
const { hidden } = useModules()

// Emergency stop: ends what plays and cancels what waits on the presentation queue; records are not touched.
const stopping = ref(false)
async function stop() {
  stopping.value = true
  try {
    const { stopped, cancelled } = await api(client.POST('/api/admin/queue/stop'))
    toast.add({ severity: 'warn', summary: t('queue.stopped', { stopped: Number(stopped), cancelled }), life: 5000 })
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    stopping.value = false
  }
}

// The chat button: docks or undocks the panel on wide windows (saved), slides it in on narrower ones.
const dock = useDock()
const wideQuery = window.matchMedia(DOCK_WIDE_QUERY)
const wide = ref(wideQuery.matches)
wideQuery.addEventListener('change', () => (wide.value = wideQuery.matches))
const chatOpen = computed(() => (wide.value ? dock.open : dock.sheet))
function toggleChat() {
  if (!wide.value) {
    dock.sheet = !dock.sheet
    return
  }
  dock.open = !dock.open
  saveDock().catch((e) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e))))
}
const simulate = ref<InstanceType<typeof SimulateDialog>>()
const simulateButton = ref<{ $el: HTMLElement }>()
const groups = computed(() => GROUPS.filter((g) => g.pages.some((p) => !hidden.value.has(p.path))))
// The user's tab bar: pinned groups in their order are in the bar; the others are always in "More".
const tabBar = useTabBar()
const editor = ref<InstanceType<typeof TabBarEditor>>()
const pinned = computed(() =>
  tabBar.groups === null ? groups.value : tabBar.groups.flatMap((id) => groups.value.find((g) => g.id === id) ?? []),
)
const unpinned = computed(() => groups.value.filter((g) => !pinned.value.includes(g)))
// The second row: categories with a page left (grey when none of them is built), or a group's own pages. A
// category is the current one while one of its pages is open; it is the page itself when that is its only page.
const subItems = computed<{ key: string; label: string; to?: string; current?: 'page' | 'true' }[]>(() => {
  const g = group.value
  if (!g) return []
  if (!g.categories)
    return g.pages
      .filter((p) => !hidden.value.has(p.path))
      .map((p) => ({ key: p.path, label: t(p.label), to: isGrey(p) ? undefined : p.path, current: p.path === route.path ? 'page' : undefined }))
  return g.categories.flatMap((c) => {
    const pages = c.pages.filter((p) => !hidden.value.has(p.path))
    if (!pages.length) return []
    const open = pages.some((p) => p.path === route.path)
    return [{
      key: c.id,
      label: t(`nav.categories.${c.id}`),
      to: categoryTarget(c, hidden.value, lastInCategory[c.id]),
      current: open ? (pages.length === 1 ? ('page' as const) : ('true' as const)) : undefined,
    }]
  })
})
const target = (g: TabGroup) => {
  const last = lastPage[g.id]
  return last && !hidden.value.has(last) ? last : firstPage(g, hidden.value)
}
// A one-page group is the page itself; a group with sub-tabs is only the current group, its sub-tab is the page.
const groupCurrent = (g: TabGroup) => (g !== group.value ? undefined : g.pages.length === 1 ? 'page' : 'true')

// Tabs that do not fit go into "More"; with room for all of them there is no "More".
const tabsEl = ref<HTMLElement>()
const moreEl = ref<HTMLElement>()
const editEl = ref<HTMLElement>()
const moreMenu = ref<InstanceType<typeof Menu>>()
const moreOpen = ref(false)
// Picking an item removes the menu, which drops focus; it goes back to "More" unless the item moved it (the tab
// editor dialog).
function onMoreHide() {
  moreOpen.value = false
  void nextTick(() => {
    if (!document.activeElement || document.activeElement === document.body) moreEl.value?.focus()
  })
}
const shown = ref(GROUPS.length)
const measuring = ref(false)
const inMore = computed(() => [...pinned.value.slice(shown.value), ...unpinned.value])
const needMore = computed(() => inMore.value.length > 0)
const currentInMore = computed(() => !!group.value && inMore.value.includes(group.value))
const moreItems = computed<MenuItem[]>(() => [
  ...inMore.value.map((g) => ({
    label: t(`nav.groups.${g.id}`),
    icon: g.icon,
    class: g === group.value ? 'current' : undefined,
    disabled: !groupUsable(g, hidden.value),
    command: () => void router.push(target(g)),
  })),
  { separator: true },
  { label: t('nav.edit'), icon: 'pi pi-pencil', command: () => editor.value?.open() },
])

// Grey items also say why when hovered, like the grey tabs.
const morePt = {
  item: ({ context }: { context: { disabled: boolean } }) => (context.disabled ? { title: t('home.unbuilt') } : {}),
}

// Below 600px the tabs are a bottom bar with room for the first PHONE_TABS groups; the rest go into "More".
const PHONE_TABS = 4
const phone = window.matchMedia('(max-width: 599.98px)')

async function fit() {
  measuring.value = true
  shown.value = pinned.value.length
  await nextTick()
  const nav = tabsEl.value
  if (phone.matches) shown.value = Math.min(pinned.value.length, PHONE_TABS)
  else if (nav && moreEl.value && editEl.value) {
    const tabs = [...nav.querySelectorAll<HTMLElement>('[data-group-tab]')]
    const style = getComputedStyle(nav)
    // Exact widths: offsetWidth rounds, and the rounding adds up over six tabs.
    const width = (el: HTMLElement) => el.getBoundingClientRect().width
    shown.value = fittingTabs(
      tabs.map(width),
      parseFloat(style.columnGap) || 0,
      nav.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight),
      width(moreEl.value),
      width(editEl.value),
    )
  }
  measuring.value = false
}

// Sub-tabs scroll sideways in one row; a fade at the right edge says there is more until the end is reached.
const subTabsEl = ref<HTMLElement>()
const subAtEnd = ref(true)
function checkSubEnd() {
  const el = subTabsEl.value
  subAtEnd.value = !el || el.scrollLeft + el.clientWidth >= el.scrollWidth - 1
}
watch(group, () => void nextTick(checkSubEnd), { flush: 'post' })
watch([pinned, () => tabBar.position], () => void fit())

const bar = ref<HTMLElement>()
const scrolled = ref(false)
const onScroll = () => (scrolled.value = window.scrollY > 0)
let timer: number | undefined
let barObserver: ResizeObserver | undefined
let tabsObserver: ResizeObserver | undefined

onMounted(() => {
  void pollTwitch()
  timer = window.setInterval(pollTwitch, 2000)
  window.addEventListener('scroll', onScroll, { passive: true })
  onScroll()
  barObserver = new ResizeObserver(() =>
    document.documentElement.style.setProperty('--appbar-h', `${bar.value?.offsetHeight ?? 0}px`),
  )
  if (bar.value) barObserver.observe(bar.value)
  tabsObserver = new ResizeObserver(() => {
    moreMenu.value?.hide()
    void fit()
    checkSubEnd()
  })
  if (tabsEl.value) tabsObserver.observe(tabsEl.value)
})
onBeforeUnmount(() => {
  window.clearInterval(clock)
  window.clearInterval(timer)
  window.removeEventListener('scroll', onScroll)
  barObserver?.disconnect()
  tabsObserver?.disconnect()
  document.documentElement.style.removeProperty('--appbar-h')
})
</script>

<style scoped>
.app-bar {
  /* The bar's own width picks what it shows, so anything that narrows it (a docked panel later) works the same. */
  container: top / inline-size;
  position: sticky;
  top: 0;
  z-index: 5;
  padding: 0.75rem 1rem 0;
  background: var(--p-surface-950);
  border-bottom: 1px solid var(--p-surface-800);
}

.app-bar.scrolled {
  box-shadow: 0 0.5rem 1rem -0.5rem rgb(0 0 0 / 0.5);
}

.bar-top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
}

.brand {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  margin-right: auto;
  font-size: 1.125rem;
  font-weight: 600;
}

.brand i {
  color: var(--p-primary-color);
}

/* One unit: when the row is too narrow the whole group moves to the next line, so settings does not end up alone
   on a line; only when the group is wider than the bar itself (very large text) do its buttons wrap. */
.bar-status {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  min-width: 0;
  align-items: center;
  gap: 0.5rem;
}

.estop {
  color: var(--p-red-400, #f87171);
}

.live-pill {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.125rem 0.625rem;
  border-radius: 999px;
  background: var(--p-red-950, #450a0a);
  font-size: 0.875rem;
  white-space: nowrap;
}

.live-pill .dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  background: var(--p-red-500, #ef4444);
}

.live-meta {
  color: var(--p-text-muted-color);
  font-variant-numeric: tabular-nums;
}

.live-pill.offline {
  background: var(--p-surface-900);
  color: var(--p-text-muted-color);
}

.live-pill.offline .dot {
  background: var(--p-surface-500);
}

/* Phones keep only 「直播中」. */
@container top (max-width: 37.5rem) {
  .live-meta {
    display: none;
  }
}

.on-air {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.125rem 0.5rem;
  border-radius: 999px;
  background: var(--p-red-950, #450a0a);
  color: var(--p-red-200, #fecaca);
  font-size: 0.875rem;
  white-space: nowrap;
}

.on-air::before {
  content: '';
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  background: var(--p-red-500, #ef4444);
}

.all-healthy,
.problem {
  display: none;
}

.problem {
  color: var(--p-yellow-400);
}

@container top (max-width: 70rem) {
  .healthy {
    display: none;
  }

  /* A narrower bar shows only the icon; the button keeps its name. */
  .simulate-open :deep(.p-button-label) {
    display: none;
  }

  .all-healthy {
    display: inline-flex;
  }
}

.tabs {
  display: flex;
  gap: 0.25rem;
  margin-top: 0.5rem;
  /* Until a resize has been measured, tabs that no longer fit must not widen the page; the margin keeps focus rings. */
  overflow: clip;
  overflow-clip-margin: 0.25rem;
}

.tabs > a,
.tabs > .off,
.tabs > .more {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  border: 0;
  border-bottom: 2px solid transparent;
  background: none;
  color: var(--p-text-muted-color);
  font: inherit;
  white-space: nowrap;
  text-decoration: none;
  cursor: pointer;
}

.tabs > a:hover,
.tabs > .more:hover {
  color: var(--p-text-color);
}

.tabs > a[aria-current],
.tabs > .more.current {
  border-bottom-color: var(--p-primary-color);
  color: var(--p-text-color);
}

/* Not built yet (or every built page turned off): grey and inert. surface-500 is 4.1:1 on surface-950; WCAG 1.4.3
   exempts inactive controls, and the state is also in words (title, sr-only text). */
.tabs > .off,
.sub-tabs .off {
  color: var(--p-surface-500);
  cursor: not-allowed;
}

.tabs > [hidden] {
  display: none;
}

/* The user put the group tabs at the bottom (wide windows; phones always have them there). */
/* The holder in App.vue is what sticks: a sticky element cannot leave its parent's box. */
.tabs.bottom {
  margin: 0;
  padding: 0 1rem;
  border-top: 1px solid var(--p-surface-800);
  background: var(--p-surface-950);
}

.tabs.bottom > a,
.tabs.bottom > .off,
.tabs.bottom > .more {
  border-top: 2px solid transparent;
  border-bottom: 0;
}

.tabs.bottom > a[aria-current],
.tabs.bottom > .more.current {
  border-top-color: var(--p-primary-color);
}

/* Positioned, so the sr-only text of a grey tab scrolls with the row instead of widening the page. */
.sub-tabs {
  position: relative;
  display: flex;
  gap: 0.375rem;
  padding: 0.625rem 0;
  overflow-x: auto;
  scrollbar-width: none;
  mask-image: linear-gradient(to right, #000 calc(100% - 2rem), transparent);
}

.sub-tabs.at-end {
  mask-image: none;
}

/* At the bottom: above the group tabs (teleported after them, so moved up by order). */
.sub-tabs.bottom {
  order: -1;
  padding: 0.5rem 1rem;
  border-top: 1px solid var(--p-surface-800);
  background: var(--p-surface-950);
}

.sub-tabs a,
.sub-tabs .off {
  flex: none;
  padding: 0.25rem 0.75rem;
  /* surface-500: 4.1:1 on surface-950 (WCAG 1.4.11). */
  border: 1px solid var(--p-surface-500);
  border-radius: 999px;
  color: var(--p-text-muted-color);
  white-space: nowrap;
  text-decoration: none;
}

.sub-tabs a:hover {
  color: var(--p-text-color);
}

.sub-tabs a[aria-current] {
  border-color: var(--p-primary-color);
  color: var(--p-primary-color);
}

/* Below 600px: the status lights give way to the warning button, and the group tabs become a bottom bar. */
@media (max-width: 599.98px) {
  .bar-status > .light {
    display: none;
  }

  .problem {
    display: inline-flex;
  }

  .tabs {
    position: fixed;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: 5;
    gap: 0;
    margin: 0;
    padding: 0.25rem 0.25rem calc(0.25rem + env(safe-area-inset-bottom));
    border-top: 1px solid var(--p-surface-800);
    background: var(--p-surface-900);
  }

  .tabs > a,
  .tabs > .off,
  .tabs > .more {
    flex: 1 1 0;
    min-width: 0;
    flex-direction: column;
    gap: 0.125rem;
    padding: 0.375rem 0.25rem;
    border-bottom: 0;
    font-size: 0.75rem;
  }

  .tabs > a i,
  .tabs > .off i,
  .tabs > .more i {
    font-size: 1.125rem;
  }

  .tabs > a[aria-current],
  .tabs > .more.current {
    color: var(--p-primary-color);
  }

  /* The category row scrolls edge to edge; its first item lines up with the page. */
  .sub-tabs {
    margin: 0 -1rem;
    padding-inline: 1rem;
    scroll-padding-inline: 1rem;
  }
}
</style>
