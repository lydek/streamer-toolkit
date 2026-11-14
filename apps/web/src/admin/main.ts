import 'primeicons/primeicons.css'
import './style.css'
import PrimeVue from 'primevue/config'
import ToastService from 'primevue/toastservice'
import { createApp } from 'vue'
import { i18n } from '../i18n'
import App from './App.vue'
import { router } from './router'
import { theme } from './theme'

createApp(App)
  .use(i18n)
  .use(router)
  // Dark only (MVP): admin/index.html always carries the class.
  // `locale` is merged into PrimeVue's defaults; only the strings our components read aloud are translated.
  .use(PrimeVue, {
    theme: { preset: theme, options: { darkModeSelector: '.app-dark' } },
    locale: { aria: { close: i18n.global.t('app.close') } },
  })
  .use(ToastService)
  .mount('#app')
