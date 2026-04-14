import { createApp } from 'vue'

import App from './App.vue'
import { createAppRouter } from './router'
import './styles/index.css'

createApp(App)
  .use(createAppRouter())
  .mount('#app')
