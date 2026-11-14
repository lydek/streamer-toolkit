import { createRouter, createWebHistory } from 'vue-router'
import { useDrawer } from './drawer'
import HomeView from './HomeView.vue'
import NotFoundView from './NotFoundView.vue'

// Base /admin/: the Host serves /admin/* with an SPA fallback, and the dev server does the same.
export const router = createRouter({
  history: createWebHistory('/admin/'),
  routes: [
    { path: '/', component: HomeView },
    { path: '/:path(.*)*', component: NotFoundView },
  ],
})

// Leaving the page (incl. "Open full page") submits the open drawer's changed settings; a failed one keeps
// the drawer and cancels the navigation so its error stays visible.
router.beforeEach(() => useDrawer().close())
