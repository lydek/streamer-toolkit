import { createApp } from 'vue'
import { i18n } from '../../i18n'
import AlertsOverlay from './AlertsOverlay.vue'

createApp(AlertsOverlay).use(i18n).mount('#app')
