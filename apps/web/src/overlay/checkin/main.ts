import { createApp } from 'vue'
import { i18n } from '../../i18n'
import CheckinOverlay from './CheckinOverlay.vue'

createApp(CheckinOverlay).use(i18n).mount('#app')
