import { createApp } from 'vue'
import { i18n } from '../../i18n'
import LeaderboardOverlay from './LeaderboardOverlay.vue'

createApp(LeaderboardOverlay).use(i18n).mount('#app')
