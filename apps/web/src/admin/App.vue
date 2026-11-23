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
import { measureUnmeasured } from './sounds'

// The saved text size applies to the whole window.
const textSize = useTextSize()
watchEffect(() => applyTextSize(textSize.value))

// Sounds imported before their length was kept get measured once; failures are retried the next time.
void measureUnmeasured().catch(() => {})
</script>

<style>
/* Pages on the left, the docked chat panel (when open on a wide window) on the right. */
.app-shell {
  display: flex;
  align-items: flex-start;
}

/* At least the window's height, so the bottom tabs sit at the window bottom even on a short page. */
.app-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  min-height: 100vh;
}

/* A page sizes by its container (`container: inline-size`, so no width of its own) and centers with auto margins,
   which in this column would shrink it to nothing: it takes the full width, up to its max-width. */
.app-main > main {
  box-sizing: border-box;
  width: 100%;
}

/* Holds the group tabs (and the group's sub-tabs above them) when the user put them at the bottom. */
#bottom-tabs {
  position: sticky;
  bottom: 0;
  z-index: 5;
  display: flex;
  flex-direction: column;
  margin-top: auto;
}

/* Below 600px the group tabs are a fixed bar (AppBar.vue); the sub-tabs stay just above it. */
@media (max-width: 599.98px) {
  #bottom-tabs {
    bottom: calc(4rem + env(safe-area-inset-bottom));
  }
}
</style>
