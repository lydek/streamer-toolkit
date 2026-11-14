import { createRouter, createWebHistory } from 'vue-router'
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
