import { createRouter, createWebHistory } from 'vue-router'
import AlertsPage from './AlertsPage.vue'
import ChatPage from './ChatPage.vue'
import CheckinsPage from './CheckinsPage.vue'
import { useDrawer } from './drawer'
import HomeView from './HomeView.vue'
import NotFoundView from './NotFoundView.vue'
import RedemptionsPage from './RedemptionsPage.vue'
import { rememberOpener } from './page'
import SettingsView from './SettingsView.vue'

// Base /admin/: the Host serves /admin/* with an SPA fallback, and the dev server does the same.
export const router = createRouter({
  history: createWebHistory('/admin/'),
  routes: [
    { path: '/', component: HomeView },
    { path: '/settings', component: SettingsView },
    { path: '/chat', component: ChatPage },
    { path: '/alerts', component: AlertsPage },
    { path: '/redemptions', component: RedemptionsPage },
    { path: '/checkin', component: CheckinsPage },
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
