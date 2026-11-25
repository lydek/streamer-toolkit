<template>
  <!-- 歌單: the songs viewers can request and their YouTube links. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <div class="song-list">
      <section class="tbl-wrap" aria-labelledby="sl-songs">
        <div class="tbl-top">
          <h3 id="sl-songs" class="tbl-head">{{ t('pages.songList.name') }}</h3>
          <Button :label="t('songList.reload')" icon="pi pi-refresh" severity="secondary" disabled />
        </div>
        <p class="empty-cell">{{ t('songList.empty') }}</p>
      </section>
      <section class="tbl-wrap" aria-labelledby="sl-links">
        <div class="tbl-top">
          <h3 id="sl-links" class="tbl-head">{{ t('songList.links') }}</h3>
          <Button :label="t('songList.findLinks')" severity="secondary" disabled />
        </div>
        <p class="empty-cell">{{ t('songList.noLinks') }}</p>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const { t } = useI18n()
</script>

<style scoped>
.song-list {
  display: grid;
  gap: 1rem;
  max-width: 72rem;
}

.song-list .tbl-wrap {
  margin-top: 0;
}
</style>
