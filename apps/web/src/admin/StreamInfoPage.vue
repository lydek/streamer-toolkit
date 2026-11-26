<template>
  <!-- 直播資訊: the title, category, tags and language viewers see on Twitch, read from it and written back. -->
  <FullPage :status="t('streamInfo.summary')">
    <div class="stream-info">
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
      <FormPanel :heading="t('streamInfo.now')">
        <FormRow :label="t('streamInfo.title')" input-id="si-title">
          <InputText id="si-title" v-model="title" class="long" :maxlength="TITLE_MAX" :disabled="!loaded" />
          <template #hint>{{ [...title].length }} / {{ TITLE_MAX }}</template>
        </FormRow>
        <FormRow :label="t('streamInfo.category')" input-id="si-category">
          <AutoComplete
            v-model="category"
            input-id="si-category"
            class="long"
            :suggestions="categories"
            option-label="name"
            :disabled="!loaded"
            :placeholder="t('streamInfo.categoryHint')"
            @complete="search($event.query)"
          />
        </FormRow>
        <FormRow :label="t('streamInfo.tags')" input-id="si-tags" top>
          <div class="tags">
            <span v-for="tag in tags" :key="tag" class="tag">
              {{ tag }}
              <button type="button" :aria-label="t('streamInfo.removeTag', { tag })" @click="tags = tags.filter((x) => x !== tag)">×</button>
            </span>
            <InputText
              id="si-tags"
              v-model="newTag"
              :placeholder="t('streamInfo.addTag')"
              :maxlength="TAG_MAX_CHARS"
              :disabled="!loaded || tags.length >= TAGS_MAX"
              @keydown.enter.prevent="addTag"
            />
          </div>
          <template #hint>{{ t('streamInfo.tagsHint', { max: TAGS_MAX }) }}</template>
        </FormRow>
        <FormRow :label="t('streamInfo.language')" input-id="si-language">
          <Select
            v-model="language"
            input-id="si-language"
            :options="languages"
            option-label="label"
            option-value="value"
            :disabled="!loaded"
          />
        </FormRow>
        <template #foot>
          <Button :label="t('streamInfo.update')" :disabled="!loaded || busy || !title.trim()" @click="save" />
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
import AutoComplete from 'primevue/autocomplete'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'

// Twitch's limits, checked again by the Host.
const TITLE_MAX = 140
const TAGS_MAX = 10
const TAG_MAX_CHARS = 25
const LANGUAGES = ['zh', 'en', 'ja', 'ko']

const { t, te } = useI18n()
const toast = useToast()
const loaded = ref(false)
const loadFailed = ref('')
const busy = ref(false)
const title = ref('')
// While typing, the box holds the text; only a category picked from the list is saved.
const category = ref<Schemas['Category'] | string | null>(null)
const categories = ref<Schemas['Category'][]>([])
const tags = ref<string[]>([])
const newTag = ref('')
const language = ref('zh')
// The channel's own language stays a choice even when it is not one of the usual ones.
const languages = computed(() =>
  [...new Set([...LANGUAGES, language.value])].map((value) => ({
    value,
    label: te(`streamInfo.languages.${value}`) ? t(`streamInfo.languages.${value}`) : value,
  })),
)

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))

function addTag() {
  const tag = newTag.value.trim()
  if (tag && !tags.value.includes(tag) && tags.value.length < TAGS_MAX) tags.value = [...tags.value, tag]
  newTag.value = ''
}

// Answers can come back out of order; only the latest search fills the list.
let searchSeq = 0
async function search(query: string) {
  const seq = ++searchSeq
  try {
    const found = await api(client.GET('/api/admin/stream-info/categories', { params: { query: { query } } }))
    if (seq === searchSeq) categories.value = found
  } catch {
    if (seq === searchSeq) categories.value = []
  }
}

async function save() {
  // A tag typed but not yet added with Enter is still meant to be saved.
  addTag()
  const picked = category.value
  if (typeof picked === 'string' && picked.trim()) {
    toast.add(errorToast(t('streamInfo.pickCategory')))
    return
  }
  busy.value = true
  try {
    await api(
      client.PUT('/api/admin/stream-info', {
        body: {
          title: title.value.trim(),
          category_id: typeof picked === 'object' && picked ? picked.id : '',
          tags: tags.value,
          language: language.value,
        },
      }),
    )
    toast.add(successToast(t('streamInfo.updated')))
  } catch (e) {
    toast.add(errorToast(message(e)))
  } finally {
    busy.value = false
  }
}

onMounted(async () => {
  try {
    const info = await api(client.GET('/api/admin/stream-info'))
    title.value = info.title
    category.value = info.category ?? null
    tags.value = info.tags
    language.value = info.language || 'zh'
    loaded.value = true
  } catch (e) {
    loadFailed.value = message(e)
  }
})
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

.tags {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.375rem;
}

.tag {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.125rem 0.25rem 0.125rem 0.625rem;
  border-radius: 999px;
  background: var(--p-surface-800);
}

.tag button {
  border: 0;
  background: none;
  color: inherit;
  cursor: pointer;
}
</style>
