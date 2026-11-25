<template>
  <!-- 點歌: the song on now, the queue, adding a song and the singing booth. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <div class="song-board">
      <div class="main">
        <section class="tbl-wrap" aria-labelledby="song-now">
          <h3 id="song-now" class="tbl-head">{{ t('songs.now') }}</h3>
          <p class="empty-cell">{{ t('songs.nothingNow') }}</p>
          <div class="tbl-foot">
            <span></span>
            <span class="actions">
              <Button :label="t('songs.skip')" severity="secondary" disabled />
              <Button :label="t('songs.next')" disabled />
            </span>
          </div>
        </section>
        <section class="tbl-wrap" aria-labelledby="song-queue">
          <div class="tbl-top">
            <h3 id="song-queue" class="tbl-head">{{ t('songs.queue') }}</h3>
            <SelectButton
              v-model="shown"
              :options="SHOWN"
              :option-label="(s: string) => t(`songs.shown.${s}`)"
              :allow-empty="false"
              :aria-label="t('songs.shownLabel')"
            />
          </div>
          <p class="empty-cell">{{ t('songs.emptyQueue') }}</p>
        </section>
      </div>
      <aside class="side">
        <FormPanel :heading="t('songs.add')">
          <FormRow :label="t('songs.search')" input-id="song-search">
            <InputText id="song-search" />
          </FormRow>
          <FormRow :label="t('songs.for')" input-id="song-for">
            <InputText id="song-for" :placeholder="t('songs.forHint')" />
          </FormRow>
        </FormPanel>
        <section class="tbl-wrap" aria-labelledby="song-booth">
          <h3 id="song-booth" class="tbl-head">{{ t('pages.singingBooth.name') }}</h3>
          <p class="empty-cell">{{ t('songs.boothOff') }}</p>
          <div class="tbl-foot">
            <span></span>
            <Button :label="t('songs.prepare')" severity="secondary" disabled />
          </div>
        </section>
      </aside>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import SelectButton from 'primevue/selectbutton'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const SHOWN = ['all', 'review']

const { t } = useI18n()
const shown = ref('all')
</script>

<style scoped>
.song-board {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(min(18rem, 100%), 24rem);
  gap: 1rem;
  align-items: start;
  max-width: 90rem;
}

.main,
.side {
  display: grid;
  gap: 1rem;
  min-width: 0;
}

.song-board .tbl-wrap {
  margin-top: 0;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

@container view (max-width: 50rem) {
  .song-board {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
