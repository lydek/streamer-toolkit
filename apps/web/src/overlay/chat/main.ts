import { createApp } from 'vue'
import { i18n } from '../../i18n'
import ChatOverlay from './ChatOverlay.vue'

createApp(ChatOverlay).use(i18n).mount('#app')
