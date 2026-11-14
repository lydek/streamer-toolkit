// Drawer test page for drawer.e2e.ts: drawer behavior no real feature uses yet (settings, a dropdown,
// "Open full page"). Served by the Vite dev server only; never part of the build (vite.config.ts lists the inputs).
import 'primeicons/primeicons.css'
import '../../src/admin/style.css'
import PrimeVue from 'primevue/config'
import { createApp, h } from 'vue'
import { RouterView, createRouter, createWebHashHistory } from 'vue-router'
import { useDrawer } from '../../src/admin/drawer'
import { theme } from '../../src/admin/theme'
import { i18n } from '../../src/i18n'
import DrawerHarness from './DrawerHarness.vue'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: DrawerHarness },
    { path: '/full', component: { render: () => h('h1', 'Full page') } },
  ],
})
// The same guard as src/admin/router.ts.
router.beforeEach(() => useDrawer().close())

createApp({ render: () => h(RouterView) })
  .use(i18n)
  .use(router)
  .use(PrimeVue, { theme: { preset: theme, options: { darkModeSelector: '.app-dark' } } })
  .mount('#app')
