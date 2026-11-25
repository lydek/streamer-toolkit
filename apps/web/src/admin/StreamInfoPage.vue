<template>
  <!-- 直播資訊: the title, category and tags viewers see on Twitch. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('streamInfo.summary')">
    <div class="stream-info">
      <FormPanel :heading="t('streamInfo.now')">
        <FormRow :label="t('streamInfo.title')" input-id="si-title">
          <InputText id="si-title" v-model="title" class="long" maxlength="140" />
          <template #hint>{{ title.length }} / 140</template>
        </FormRow>
        <FormRow :label="t('streamInfo.category')" input-id="si-category">
          <InputText id="si-category" class="long" />
        </FormRow>
        <FormRow :label="t('streamInfo.tags')" input-id="si-tags">
          <InputText id="si-tags" :placeholder="t('streamInfo.addTag')" />
        </FormRow>
        <FormRow :label="t('streamInfo.language')" input-id="si-language">
          <Select v-model="language" input-id="si-language" :options="LANGUAGES" />
        </FormRow>
        <template #foot>
          <Button :label="t('streamInfo.update')" disabled />
        </template>
      </FormPanel>

      <section class="tbl-wrap" aria-labelledby="si-templates">
        <h3 id="si-templates" class="tbl-head">{{ t('streamInfo.templates.heading') }}</h3>
        <p class="empty-cell">{{ t('streamInfo.templates.empty') }}</p>
        <div class="tbl-foot"><span class="note">{{ t('streamInfo.scope') }}</span></div>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const { t } = useI18n()
const title = ref('')
const LANGUAGES = ['中文', 'English', '日本語']
const language = ref(LANGUAGES[0])
</script>

<style scoped>
.stream-info {
  display: grid;
  gap: 1rem;
  max-width: 60rem;
}

.long {
  width: 20rem;
  max-width: 100%;
}
</style>
