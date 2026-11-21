import { createRouter, createWebHistory } from 'vue-router'
import AlertsPage from './AlertsPage.vue'
import CardStylePage from './CardStylePage.vue'
import ChatLogPage from './ChatLogPage.vue'
import ChatPage from './ChatPage.vue'
import CheckinsPage from './CheckinsPage.vue'
import { useDrawer } from './drawer'
import HomeView from './HomeView.vue'
import LeaderboardPage from './LeaderboardPage.vue'
import CountersPage from './CountersPage.vue'
import MusicPage from './MusicPage.vue'
import NotFoundView from './NotFoundView.vue'
import RedemptionsPage from './RedemptionsPage.vue'
import RehearsalPage from './RehearsalPage.vue'
import { rememberOpener } from './page'
import SettingsView from './SettingsView.vue'
import ScenesPage from './ScenesPage.vue'
import ScreenToolsPage from './ScreenToolsPage.vue'
import SoundsPage from './SoundsPage.vue'
import { SOON_PAGES } from './tabs'

// Base /admin/: the Host serves /admin/* with an SPA fallback, and the dev server does the same.
export const router = createRouter({
  history: createWebHistory('/admin/'),
  routes: [
    { path: '/', component: HomeView },
    { path: '/settings', redirect: '/settings/general' },
    { path: '/settings/:section(general|connections|features|profiles|hotkeys|debug|backup|about)', component: SettingsView },
    { path: '/chat-log', component: ChatLogPage },
    { path: '/chat', component: ChatPage },
    { path: '/alerts', component: AlertsPage },
    { path: '/redemptions', component: RedemptionsPage },
    { path: '/checkin', component: CheckinsPage },
    { path: '/card-style', component: CardStylePage },
    { path: '/sounds', component: SoundsPage },
    { path: '/scenes', component: ScenesPage },
    { path: '/music', component: MusicPage },
    { path: '/counters', component: CountersPage },
    { path: '/rehearsal', component: RehearsalPage },
    { path: '/leaderboard', component: LeaderboardPage },
    { path: '/screen-tools', component: ScreenToolsPage },
    // Screens of the design that are not built yet (docs/scope.md 階段 0).
    // A page not built yet has a grey tab; its URL leads home.
    ...SOON_PAGES.map((p) => ({ path: p.path, redirect: '/' })),
    { path: '/:path(.*)*', component: NotFoundView },
  ],
})

// Leaving the page (incl. "Open full page") submits the open drawer's or the full page's changed settings; a
// failed one keeps them and cancels the navigation so its error stays visible.
router.beforeEach(async (to, from) => {
  const focused = document.activeElement
  if (!(await useDrawer().close())) return false
  rememberOpener(to, from, focused)
})
