import { createApp } from 'vue'
import { i18n } from '../../i18n'
import PollsOverlay from './PollsOverlay.vue'

createApp(PollsOverlay).use(i18n).mount('#app')
