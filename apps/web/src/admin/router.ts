import { createRouter, createWebHistory } from 'vue-router'
import { useDrawer } from './drawer'
import { modulesLoaded, useModules } from './features'
import HomeView from './HomeView.vue'
import { rememberOpener } from './page'
import { needsSetup } from './setup'
import { SOON_PAGES, closedPageTarget, showUnbuilt } from './tabs'

// Unbuilt pages drawn ahead of their backend (UI-G), opened in dev mode only; lazy, so release never loads them.
export const DRAWN: Record<string, () => Promise<unknown>> = {
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
  // Every page but Home loads on its first visit, so the admin window starts with Home and the shell only.
  routes: [
    { path: '/', component: HomeView },
    { path: '/setup', component: () => import('./SetupPage.vue') },
    { path: '/settings', redirect: '/settings/general' },
    { path: '/settings/:section(general|connections|features|profiles|hotkeys|debug|backup|about)', component: () => import('./SettingsView.vue') },
    { path: '/chat-log', component: () => import('./ChatLogPage.vue') },
    { path: '/chat', component: () => import('./ChatPage.vue') },
    { path: '/alerts', component: () => import('./AlertsPage.vue') },
    { path: '/alert-events', component: () => import('./AlertEventsPage.vue') },
    { path: '/alert-log', component: () => import('./AlertLogPage.vue') },
    { path: '/redemptions', component: () => import('./RedemptionsPage.vue') },
    { path: '/reward-playback', component: () => import('./RewardPlaybackPage.vue') },
    { path: '/checkin', component: () => import('./CheckinsPage.vue') },
    { path: '/card-style', component: () => import('./CardStylePage.vue') },
    { path: '/checkin-rules', component: () => import('./CheckinRulesPage.vue') },
    { path: '/sounds', component: () => import('./SoundsPage.vue') },
    { path: '/scenes', component: () => import('./ScenesPage.vue') },
    { path: '/mixer', component: () => import('./MixerPage.vue') },
    { path: '/obs-sources', component: () => import('./ObsSourcesPage.vue') },
    { path: '/music', component: () => import('./MusicPage.vue'), props: { part: 'player' } },
    { path: '/music-playlists', component: () => import('./MusicPage.vue'), props: { part: 'playlists' } },
    { path: '/music-library', component: () => import('./MusicPage.vue'), props: { part: 'library' } },
    { path: '/counters', component: () => import('./CountersPage.vue') },
    { path: '/marathon', component: () => import('./MarathonPage.vue') },
    { path: '/rehearsal', component: () => import('./RehearsalPage.vue') },
    { path: '/leaderboard', component: () => import('./LeaderboardPage.vue') },
    { path: '/image-sizes', component: () => import('./ImageSizesPage.vue') },
    { path: '/screen-tools', component: () => import('./ScreenToolsPage.vue') },
    { path: '/warnings', component: () => import('./WarningsPage.vue') },
    { path: '/overlay-look', component: () => import('./OverlayLookPage.vue') },
    { path: '/media', component: () => import('./MediaPage.vue') },
    { path: '/stream-info', component: () => import('./StreamInfoPage.vue') },
    { path: '/rewards', component: () => import('./RewardsPage.vue') },
    { path: '/polls', component: () => import('./PollsPage.vue') },
    { path: '/songs', component: () => import('./SongsPage.vue') },
    { path: '/song-list', component: () => import('./SongListPage.vue') },
    { path: '/commands', component: () => import('./BotCommandsPage.vue') },
    { path: '/timers', component: () => import('./BotTimersPage.vue') },
    { path: '/greetings', component: () => import('./BotGreetingsPage.vue') },
    // Screens of the design that are not built yet (docs/scope.md 階段 0): their URL leads home, except in dev mode,
    // where a page drawn ahead of its backend opens (DRAWN) and the rest show a placeholder.
    ...SOON_PAGES.map((p) => ({
      path: p.path,
      component: DRAWN[p.path] ?? (() => import('./UnbuiltPage.vue')),
      beforeEnter: () => showUnbuilt() || '/',
    })),
    { path: '/:path(.*)*', component: () => import('./NotFoundView.vue') },
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
