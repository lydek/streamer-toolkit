<template>
  <!-- 歌單: the songs viewers can request, their categories, and importing a spreadsheet (docs/modules/songs.md). -->
  <FullPage :status="status">
    <div class="song-list">
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>

      <section class="tbl-wrap" :aria-label="t('pages.songList.name')">
        <div class="tbl-top">
          <InputText v-model="query" type="search" :placeholder="t('songList.search')" :aria-label="t('songList.search')" />
          <div class="tools">
            <Button :label="t('songList.manageCategories')" icon="pi pi-tags" severity="secondary" @click="categoryDialog?.showModal()" />
            <Button :label="t('songList.import')" icon="pi pi-upload" severity="secondary" @click="fileInput?.click()" />
            <Button :label="t('songList.add')" icon="pi pi-plus" @click="edit()" />
          </div>
        </div>
        <input ref="fileInput" type="file" accept=".xlsx,.csv" hidden data-import @change="pick" />
        <div v-if="categories.length" class="chips" role="group" :aria-label="t('songList.openCategories')">
          <span class="muted">{{ t('songList.openCategories') }}</span>
          <Button
            v-for="c in categories"
            :key="c.id"
            :label="t('songList.chip', { name: c.name, n: c.songs })"
            :icon="c.requestable ? 'pi pi-check' : undefined"
            size="small"
            :severity="c.requestable ? undefined : 'secondary'"
            :outlined="!c.requestable"
            :aria-pressed="c.requestable"
            :data-category="c.id"
            @click="saveCategory(c, c.name, !c.requestable)"
          />
        </div>
        <p v-if="!songs.length" class="empty-cell">{{ t('songList.empty') }}</p>
        <table v-else class="table">
          <thead>
            <tr>
              <th scope="col">{{ t('songList.title') }}</th>
              <th scope="col">{{ t('songList.artist') }}</th>
              <th scope="col">{{ t('songList.category') }}</th>
              <th scope="col">{{ t('songList.lastSung') }}</th>
              <th scope="col" class="num">{{ t('songList.requested') }}</th>
              <th scope="col"><span class="sr-only">{{ t('songList.actions') }}</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="s in visible" :key="s.id" :data-song="s.id">
              <th scope="row">
                {{ s.title }}
                <i v-if="s.youtube_url" class="pi pi-youtube muted" role="img" :aria-label="t('songList.youtube')" :title="s.youtube_url" />
              </th>
              <td>{{ s.artist || '—' }}</td>
              <td>{{ categoryName(s.category_id) }}</td>
              <td>{{ lastSung(s.last_sung_at) }}</td>
              <td class="num">{{ s.request_count }}</td>
              <td class="num">
                <Button icon="pi pi-pencil" text :aria-label="t('songList.editLabel', { title: s.title })" @click="edit(s)" />
              </td>
            </tr>
          </tbody>
        </table>
        <div class="tbl-foot"><span class="note">{{ t('songList.links', { n: songs.length, linked }) }}</span></div>
      </section>
    </div>

    <dialog ref="categoryDialog" class="sheet category-dialog" aria-labelledby="category-title">
      <h2 id="category-title">{{ t('songList.manageCategories') }}</h2>
      <ul class="categories">
        <li v-for="c in categories" :key="c.id" :data-category="c.id">
          <InputText
            :model-value="c.name"
            :maxlength="CATEGORY_MAX"
            :aria-label="t('songList.categoryName', { name: c.name })"
            @change="renameCategory(c, ($event.target as HTMLInputElement).value)"
          />
          <span class="muted">{{ t('songList.count', { n: c.songs }) }}</span>
          <Button icon="pi pi-trash" text :aria-label="t('songList.deleteCategory', { name: c.name })" @click="deleteCategory(c)" />
        </li>
      </ul>
      <form class="add-category" @submit.prevent="addCategory">
        <InputText v-model="newCategory" :maxlength="CATEGORY_MAX" :placeholder="t('songList.newCategory')" :aria-label="t('songList.newCategory')" />
        <Button :label="t('songList.addCategory')" icon="pi pi-plus" type="submit" text :disabled="!newCategory.trim()" />
      </form>
      <div class="buttons sheet-actions">
        <Button :label="t('songList.done')" @click="categoryDialog?.close()" />
      </div>
    </dialog>

    <dialog ref="songDialog" class="sheet song-dialog" aria-labelledby="song-title">
      <h2 id="song-title">{{ t(editing ? 'songList.editTitle' : 'songList.addTitle') }}</h2>
      <form id="song-form" @submit.prevent="saveSong">
        <FormRow :label="t('songList.title')" input-id="song-name">
          <InputText id="song-name" v-model="draft.title" :maxlength="TITLE_MAX" />
        </FormRow>
        <FormRow :label="t('songList.artist')" input-id="song-artist">
          <InputText id="song-artist" v-model="draft.artist" :maxlength="TITLE_MAX" />
        </FormRow>
        <FormRow :label="t('songList.category')" input-id="song-category">
          <Select v-model="draft.category_id" input-id="song-category" :options="categoryOptions" option-label="label" option-value="value" />
        </FormRow>
        <FormRow :label="t('songList.youtube')" input-id="song-link">
          <InputText id="song-link" v-model="draft.youtube_url" />
        </FormRow>
      </form>
      <p v-if="failed" class="failed" role="alert">{{ failed }}</p>
      <div class="buttons sheet-actions">
        <Button v-if="editing" :label="t('songList.delete')" icon="pi pi-trash" severity="danger" text class="delete" @click="deleteSong" />
        <Button :label="t('songList.save')" type="submit" form="song-form" :disabled="busy || !draft.title.trim()" />
        <Button :label="t('features.cancel')" text @click="songDialog?.close()" />
      </div>
    </dialog>

    <dialog ref="importDialog" class="sheet import-dialog" aria-labelledby="import-title">
      <h2 id="import-title">{{ t('songList.importTitle', { file: picked?.name }) }}</h2>
      <template v-if="preview">
        <p role="status" data-preview>{{ t('songList.importSummary', { added: preview.added, updated: preview.updated, unchanged: preview.unchanged }) }}</p>
        <p v-if="preview.new_categories.length">{{ t('songList.importCategories', { names: preview.new_categories.join('、') }) }}</p>
        <ul v-if="preview.skipped.length" class="skipped">
          <li v-for="(s, i) in preview.skipped" :key="i">{{ skipText(s) }}</li>
        </ul>
      </template>
      <p v-if="failed" class="failed" role="alert">{{ failed }}</p>
      <div class="buttons sheet-actions">
        <Button :label="t('songList.importOk')" :disabled="busy || !preview || !(preview.added + preview.updated)" @click="applyImport" />
        <Button :label="t('features.cancel')" text @click="importDialog?.close()" />
      </div>
    </dialog>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'

type Song = Schemas['SongView']
type Category = Schemas['CategoryView']
type ImportResult = Schemas['ImportResult']
// The Host's limits, checked again there.
const TITLE_MAX = 100
const CATEGORY_MAX = 30
const IMPORT_MAX_BYTES = 5 * 1024 * 1024
const UNCATEGORISED = ''

const { t } = useI18n()
const toast = useToast()
const songs = ref<Song[]>([])
const categories = ref<Category[]>([])
const loadFailed = ref('')
const busy = ref(false)
const failed = ref('')
const query = ref('')
const newCategory = ref('')
const songDialog = ref<HTMLDialogElement>()
const categoryDialog = ref<HTMLDialogElement>()
const importDialog = ref<HTMLDialogElement>()
const fileInput = ref<HTMLInputElement>()
const editing = ref<Song>()
const picked = ref<File>()
const preview = ref<ImportResult>()
const draft = reactive({ title: '', artist: '', category_id: UNCATEGORISED, youtube_url: '' })

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))
const linked = computed(() => songs.value.filter((s) => s.youtube_url).length)
const status = computed(() => t('songList.summary', { n: songs.value.length }))
const categoryName = (id: string | null) => categories.value.find((c) => c.id === id)?.name ?? t('songList.uncategorised')
const categoryOptions = computed(() => [
  { value: UNCATEGORISED, label: t('songList.uncategorised') },
  ...categories.value.map((c) => ({ value: c.id, label: c.name })),
])
const visible = computed(() => {
  const q = query.value.trim().toLowerCase()
  return q ? songs.value.filter((s) => `${s.title} ${s.artist} ${categoryName(s.category_id)}`.toLowerCase().includes(q)) : songs.value
})

function lastSung(at: string | null) {
  if (!at) return t('songList.never')
  const days = Math.floor((Date.now() - Date.parse(at)) / 86_400_000)
  return days <= 0 ? t('songList.today') : t('songList.daysAgo', { n: days })
}

function skipText(s: Schemas['ImportSkipped']) {
  const where = s.sheet ? t('songList.skipWhereSheet', { sheet: s.sheet, row: s.row }) : t('songList.skipWhere', { row: s.row })
  return `${where}：${t(`songList.skip.${s.reason}`)}`
}

async function load() {
  const library = await api(client.GET('/api/admin/song-list'))
  songs.value = library.songs
  categories.value = library.categories
}

async function run(action: () => Promise<unknown>) {
  try {
    await action()
    await load()
  } catch (e) {
    toast.add(errorToast(message(e)))
    await load().catch(() => {})
  }
}

const saveCategory = (c: Category, name: string, requestable: boolean) =>
  run(() => api(client.PUT('/api/admin/song-list/categories/{id}', { params: { path: { id: c.id } }, body: { name, requestable } })))
const renameCategory = (c: Category, name: string) => (name.trim() && name !== c.name ? saveCategory(c, name, c.requestable) : undefined)
const deleteCategory = (c: Category) => run(() => api(client.DELETE('/api/admin/song-list/categories/{id}', { params: { path: { id: c.id } } })))
const addCategory = () =>
  run(async () => {
    await api(client.POST('/api/admin/song-list/categories', { body: { name: newCategory.value.trim(), requestable: true } }))
    newCategory.value = ''
  })

function edit(s?: Song) {
  editing.value = s
  Object.assign(draft, {
    title: s?.title ?? '',
    artist: s?.artist ?? '',
    category_id: s?.category_id ?? UNCATEGORISED,
    youtube_url: s?.youtube_url ?? '',
  })
  failed.value = ''
  songDialog.value!.showModal()
}

async function saveSong() {
  busy.value = true
  failed.value = ''
  const body = { title: draft.title, artist: draft.artist, category_id: draft.category_id || null, youtube_url: draft.youtube_url }
  try {
    const id = editing.value?.id
    await api(
      id
        ? client.PUT('/api/admin/song-list/songs/{id}', { params: { path: { id } }, body })
        : client.POST('/api/admin/song-list/songs', { body }),
    )
    songDialog.value?.close()
    await load()
  } catch (e) {
    failed.value = message(e)
  } finally {
    busy.value = false
  }
}

async function deleteSong() {
  const id = editing.value!.id
  try {
    await api(client.DELETE('/api/admin/song-list/songs/{id}', { params: { path: { id } } }))
    songDialog.value?.close()
    await load()
  } catch (e) {
    failed.value = message(e)
  }
}

/** Sends the picked file as itself (not JSON), for a preview or the import. */
function send(path: '/api/admin/song-list/import/preview' | '/api/admin/song-list/import', file: File) {
  const format = file.name.toLowerCase().endsWith('.xlsx') ? 'xlsx' : 'csv'
  return api(
    client.POST(path, {
      params: { query: { format } },
      body: file as unknown as number[],
      bodySerializer: (b: unknown) => b as BodyInit,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  )
}

async function pick(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  picked.value = file
  preview.value = undefined
  failed.value = ''
  importDialog.value!.showModal()
  // The Host refuses a larger body before reading it, without an error code.
  if (file.size > IMPORT_MAX_BYTES) return (failed.value = errorMessage('songs.unreadable_file'))
  try {
    preview.value = await send('/api/admin/song-list/import/preview', file)
  } catch (err) {
    failed.value = message(err)
  }
}

async function applyImport() {
  busy.value = true
  try {
    const done = await send('/api/admin/song-list/import', picked.value!)
    importDialog.value?.close()
    toast.add(successToast(t('songList.imported', { added: done.added, updated: done.updated })))
    await load()
  } catch (e) {
    failed.value = message(e)
  } finally {
    busy.value = false
  }
}

load().catch((e: unknown) => (loadFailed.value = message(e)))
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

.categories {
  display: grid;
  gap: 0.5rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.chips {
  padding: 0.5rem 1rem;
  border-bottom: 1px solid var(--p-content-border-color);
}

.categories li,
.add-category,
.chips {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.75rem;
}

.add-category {
  padding-top: 0.75rem;
}

.tools {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.muted,
.note {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.num {
  text-align: end;
}

.song-dialog,
.category-dialog,
.import-dialog {
  width: min(40rem, calc(100vw - 2rem));
}

.skipped {
  max-height: 12rem;
  overflow: auto;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.delete {
  margin-inline-end: auto;
}

.failed {
  color: var(--p-red-400);
}
</style>
