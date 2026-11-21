import { createApp } from 'vue'
import { i18n } from '../../i18n'
import CounterOverlay from './CounterOverlay.vue'

createApp(CounterOverlay).use(i18n).mount('#app')
