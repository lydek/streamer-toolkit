<template>
  <!-- 虛擬形象 › 3D 形象: VTuber Plus and VSeeFace expressions and interactions. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <div class="avatar">
      <div class="apps">
        <section v-for="a in APPS" :key="a" class="tbl-wrap" :aria-labelledby="`av-${a}`">
          <div class="tbl-top">
            <h3 :id="`av-${a}`" class="tbl-head">{{ t(`avatar.apps.${a}`) }}</h3>
            <Button :label="t('avatar.setup')" severity="secondary" size="small" disabled />
          </div>
          <p class="empty-cell">{{ t('avatar.notConnected') }}</p>
        </section>
      </div>

      <section class="tbl-wrap" aria-labelledby="av-faces">
        <h3 id="av-faces" class="tbl-head">{{ t('avatar.faces') }}</h3>
        <div class="faces">
          <Button v-for="f in FACES" :key="f" :label="t(`avatar.face.${f}`)" severity="secondary" disabled />
        </div>
      </section>

      <section class="tbl-wrap" aria-labelledby="av-items">
        <h3 id="av-items" class="tbl-head">{{ t('avatar.items') }}</h3>
        <p class="empty-cell">{{ t('avatar.noItems') }}</p>
        <div class="tbl-foot">
          <span class="note">{{ t('avatar.itemsNote') }}</span>
          <Button :label="t('avatar.addItem')" icon="pi pi-plus" disabled />
        </div>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const APPS = ['vtuberPlus', 'vseeface']
const FACES = ['joy', 'angry', 'sorrow', 'fun', 'surprised', 'neutral']

const { t } = useI18n()
</script>

<style scoped>
.avatar {
  display: grid;
  gap: 1rem;
  max-width: 90rem;
}

.avatar .tbl-wrap {
  margin-top: 0;
}

.apps {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(22rem, 100%), 1fr));
  gap: 1rem;
  align-items: start;
}

.faces {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  padding: 0.75rem;
}

.note {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
