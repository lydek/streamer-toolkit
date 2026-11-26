import { createRouter, createWebHistory } from 'vue-router'
import AlertEventsPage from './AlertEventsPage.vue'
import AlertLogPage from './AlertLogPage.vue'
import AlertsPage from './AlertsPage.vue'
import CardStylePage from './CardStylePage.vue'
import ChatLogPage from './ChatLogPage.vue'
import ChatPage from './ChatPage.vue'
import CheckinRulesPage from './CheckinRulesPage.vue'
import CheckinsPage from './CheckinsPage.vue'
import { useDrawer } from './drawer'
import { modulesLoaded, useModules } from './features'
import HomeView from './HomeView.vue'
import ImageSizesPage from './ImageSizesPage.vue'
import LeaderboardPage from './LeaderboardPage.vue'
import CountersPage from './CountersPage.vue'
import MarathonPage from './MarathonPage.vue'
import MusicPage from './MusicPage.vue'
import NotFoundView from './NotFoundView.vue'
import OverlayLookPage from './OverlayLookPage.vue'
import MixerPage from './MixerPage.vue'
import ObsSourcesPage from './ObsSourcesPage.vue'
import RedemptionsPage from './RedemptionsPage.vue'
import RehearsalPage from './RehearsalPage.vue'
import RewardPlaybackPage from './RewardPlaybackPage.vue'
import { rememberOpener } from './page'
import SettingsView from './SettingsView.vue'
import ScenesPage from './ScenesPage.vue'
import MediaPage from './MediaPage.vue'
import ScreenToolsPage from './ScreenToolsPage.vue'
import WarningsPage from './WarningsPage.vue'
import SoundsPage from './SoundsPage.vue'
import StreamInfoPage from './StreamInfoPage.vue'
import RewardsPage from './RewardsPage.vue'
import PollsPage from './PollsPage.vue'
import { needsSetup } from './setup'
import { SOON_PAGES, closedPageTarget, showUnbuilt } from './tabs'

// Unbuilt pages drawn ahead of their backend (UI-G), opened in dev mode only; lazy, so release never loads them.
export const DRAWN: Record<string, () => Promise<unknown>> = {
  '/commands': () => import('./BotCommandsPage.vue'),
  '/timers': () => import('./BotTimersPage.vue'),
  '/greetings': () => import('./BotGreetingsPage.vue'),
  '/songs': () => import('./SongsPage.vue'),
  '/song-list': () => import('./SongListPage.vue'),
  '/singing-booth': () => import('./SingingBoothPage.vue'),
  '/shoutouts': () => import('./ShoutoutsPage.vue'),
  '/raids': () => import('./RaidsPage.vue'),
  '/credits': () => import('./CreditsPage.vue'),
  '/modes': () => import('./ModesPage.vue'),
  '/viewers': () => import('./ViewersPage.vue'),
  '/roles': () => import('./RolesPage.vue'),
  '/chat-filter': () => import('./ChatFilterPage.vue'),
  '/avatar': () => import('./AvatarPage.vue'),
  '/mic-avatar': () => import('./MicAvatarPage.vue'),
  '/outfits': () => import('./OutfitsPage.vue'),
  '/captions': () => import('./CaptionsPage.vue'),
  '/goals': () => import('./GoalsPage.vue'),
  '/music-requests': () => import('./MusicRequestsPage.vue'),
  '/chat-window': () => import('./ChatWindowPage.vue'),
  '/song-rules': () => import('./SongRulesPage.vue'),
}

// Base /admin/: the Host serves /admin/* with an SPA fallback, and the dev server does the same.
export const router = createRouter({
  history: createWebHistory('/admin/'),
  routes: [
    { path: '/', component: HomeView },
    { path: '/setup', component: () => import('./SetupPage.vue') },
    { path: '/settings', redirect: '/settings/general' },
    { path: '/settings/:section(general|connections|features|profiles|hotkeys|debug|backup|about)', component: SettingsView },
    { path: '/chat-log', component: ChatLogPage },
    { path: '/chat', component: ChatPage },
    { path: '/alerts', component: AlertsPage },
    { path: '/alert-events', component: AlertEventsPage },
    { path: '/alert-log', component: AlertLogPage },
    { path: '/redemptions', component: RedemptionsPage },
    { path: '/reward-playback', component: RewardPlaybackPage },
    { path: '/checkin', component: CheckinsPage },
    { path: '/card-style', component: CardStylePage },
    { path: '/checkin-rules', component: CheckinRulesPage },
    { path: '/sounds', component: SoundsPage },
    { path: '/scenes', component: ScenesPage },
    { path: '/mixer', component: MixerPage },
    { path: '/obs-sources', component: ObsSourcesPage },
    { path: '/music', component: MusicPage, props: { part: 'player' } },
    { path: '/music-playlists', component: MusicPage, props: { part: 'playlists' } },
    { path: '/music-library', component: MusicPage, props: { part: 'library' } },
    { path: '/counters', component: CountersPage },
    { path: '/marathon', component: MarathonPage },
    { path: '/rehearsal', component: RehearsalPage },
    { path: '/leaderboard', component: LeaderboardPage },
    { path: '/image-sizes', component: ImageSizesPage },
    { path: '/screen-tools', component: ScreenToolsPage },
    { path: '/warnings', component: WarningsPage },
    { path: '/overlay-look', component: OverlayLookPage },
    { path: '/media', component: MediaPage },
    { path: '/stream-info', component: StreamInfoPage },
    { path: '/rewards', component: RewardsPage },
    { path: '/polls', component: PollsPage },
    // Screens of the design that are not built yet (docs/scope.md 階段 0): their URL leads home, except in dev mode,
    // where a page drawn ahead of its backend opens (DRAWN) and the rest show a placeholder.
    ...SOON_PAGES.map((p) => ({
      path: p.path,
      component: DRAWN[p.path] ?? (() => import('./UnbuiltPage.vue')),
      beforeEnter: () => showUnbuilt() || '/',
    })),
    { path: '/:path(.*)*', component: NotFoundView },
  ],
})

// Leaving the page (incl. "Open full page") submits the open drawer's or the full page's changed settings; a
// failed one keeps them and cancels the navigation so its error stays visible. A page whose feature is off cannot
// be opened from anywhere (tab, URL, back): it leads to an open page of its group instead.
router.beforeEach(async (to, from) => {
  const focused = document.activeElement
  if (!(await useDrawer().close())) return false
  // A data folder that never finished or skipped the first-run setup opens on it instead of home.
  if (to.path === '/' && (await needsSetup())) return '/setup'
  await modulesLoaded()
  // The matched route's path: the router also takes other letter cases and a trailing slash.
  const instead = closedPageTarget(to.matched.at(-1)?.path ?? to.path, useModules().hidden.value)
  if (instead) return instead
  rememberOpener(to, from, focused)
})
