<template>
  <!-- A page's skip link is teleported here, so it stays the first Tab stop ahead of the app bar. -->
  <div id="skip-links"></div>
  <div class="app-shell">
    <div class="app-main">
      <AppBar />
      <RouterView />
      <!-- The group tabs move here when the user puts them at the bottom. -->
      <div id="bottom-tabs"></div>
    </div>
    <ChatDock />
  </div>
  <CloseHint />
  <!-- Fits the window below 600px instead of the default 25rem width. -->
  <Toast :breakpoints="{ '600px': { width: 'calc(100vw - 2.5rem)' } }" />
</template>

<script setup lang="ts">
import Toast from 'primevue/toast'
import AppBar from './AppBar.vue'
import ChatDock from './ChatDock.vue'
import { applyTextSize, useTextSize } from './textsize'
import CloseHint from './CloseHint.vue'
import { watchEffect } from 'vue'

// The saved text size applies to the whole window.
const textSize = useTextSize()
watchEffect(() => applyTextSize(textSize.value))
</script>

<style>
/* Pages on the left, the docked chat panel (when open on a wide window) on the right. */
.app-shell {
  display: flex;
  align-items: flex-start;
}

.app-main {
  flex: 1;
  min-width: 0;
}

/* Holds the group tabs when the user put them at the bottom. */
#bottom-tabs {
  position: sticky;
  bottom: 0;
  z-index: 5;
}
</style>
