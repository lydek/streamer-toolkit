import { createApp } from 'vue'
import { i18n } from '../../i18n'
import WarningsOverlay from './WarningsOverlay.vue'

createApp(WarningsOverlay).use(i18n).mount('#app')
