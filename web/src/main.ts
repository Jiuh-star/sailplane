import { createApp } from 'vue'

import App from './App.vue'
import './assets/index.css'
import { applyStoredLocale, i18n } from './i18n'
import { router } from './router'

applyStoredLocale()

createApp(App).use(i18n).use(router).mount('#app')
