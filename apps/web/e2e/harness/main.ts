// Drawer test page for drawer.e2e.ts: drawer behavior no real feature uses yet (settings, a dropdown,
// "Open full page"). Also unbuilt pages, which are grey in Vite mode `e2e` (#/chat-filter for chat-filter.e2e.ts).
// Served by the Vite dev server only; never part of the build (vite.config.ts lists the inputs).
import 'primeicons/primeicons.css'
import '../../src/admin/style.css'
import PrimeVue from 'primevue/config'
import { createApp, h } from 'vue'
import { RouterView, createRouter, createWebHashHistory } from 'vue-router'
import { useDrawer } from '../../src/admin/drawer'
import { theme } from '../../src/admin/theme'
import ChatFilterPage from '../../src/admin/ChatFilterPage.vue'
import { i18n } from '../../src/i18n'
import DrawerHarness from './DrawerHarness.vue'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: DrawerHarness },
    { path: '/full', component: { render: () => h('h1', 'Full page') } },
    // In <main>, as in the app, so the page lays out by its own width.
    { path: '/chat-filter', component: { render: () => h('main', h(ChatFilterPage)) } },
  ],
})
// The same guard as src/admin/router.ts.
router.beforeEach(() => useDrawer().close())

createApp({ render: () => h(RouterView) })
  .use(i18n)
  .use(router)
  .use(PrimeVue, { theme: { preset: theme, options: { darkModeSelector: '.app-dark' } } })
  .mount('#app')
