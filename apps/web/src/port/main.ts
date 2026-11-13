import { createApp } from 'vue'
import { i18n } from '../i18n'
import PortPage from './PortPage.vue'

createApp(PortPage).use(i18n).mount('#app')
