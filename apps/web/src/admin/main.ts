import 'primeicons/primeicons.css'
import './style.css'
import PrimeVue from 'primevue/config'
import { createApp } from 'vue'
import { i18n } from '../i18n'
import App from './App.vue'
import { router } from './router'
import { theme } from './theme'

createApp(App)
  .use(i18n)
  .use(router)
  // Dark only (MVP): admin/index.html always carries the class.
  .use(PrimeVue, { theme: { preset: theme, options: { darkModeSelector: '.app-dark' } } })
  .mount('#app')
