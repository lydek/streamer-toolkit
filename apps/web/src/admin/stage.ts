import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue'
import { errorMessage, i18n } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { type LiveState, type Stage, clockText, preStreamChecks, sessionRange, streamStage, twitchOverview } from './home'
import { useServices } from './services'
import { exportRecord } from './streamRecord'

export const STAGES: Stage[] = ['before', 'live', 'after']
// Follows, subs and the rest arrive between status events; read them again this often while live.
const REFRESH_MS = 30_000
// How often 開播前 asks which overlays are open in OBS.
const OVERLAYS_MS = 3000

/**
 * The stream stage on home, shared by its blocks (開播前檢查, 本場直播, 本場摘要): before, live or after, following
 * Twitch; picking another stage holds until the next change from Twitch. Call it in a component's setup: its timers
 * stop with that component.
 */
export function useStage(source: { live: () => LiveState; modules: () => Schemas['ModuleState'][] | undefined }) {
  const { t } = i18n.global
  const summary = shallowRef<Schemas['StreamSummary'] | null>()
  const failed = ref('')
  async function load() {
    try {
      summary.value = await api(client.GET('/api/admin/stream/summary'))
      failed.value = ''
    } catch (e) {
      failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
    }
  }

  const auto = computed(() => streamStage(source.live().stream, summary.value))
  const picked = ref<Stage>()
  const shown = computed({ get: () => picked.value ?? auto.value, set: (s: Stage) => (picked.value = s) })
  watch(auto, () => (picked.value = undefined))
  /** Picked by hand, away from the one the stream status gives; until the next change of status. */
  const manual = computed(() => picked.value !== undefined && picked.value !== auto.value)
  const backToAuto = () => (picked.value = undefined)

  // A start, an end or a sign-in changes which session is the latest: read it again.
  watch([() => source.live().stream?.state, () => source.live().stream?.startedAt], () => void load(), { immediate: true })
  const now = ref(Date.now())
  const clock = window.setInterval(() => (now.value = Date.now()), 1000)
  const refresh = window.setInterval(() => {
    if (source.live().stream?.state === 'live') void load()
  }, REFRESH_MS)

  // The app bar keeps the Twitch status current; OBS comes with the live events; overlays are asked for here.
  const overlays = shallowRef<Schemas['OverlayCounts']>()
  async function loadOverlays() {
    if (shown.value !== 'before') return
    overlays.value = await api(client.GET('/api/admin/overlays')).catch(() => undefined)
  }
  watch(shown, () => void loadOverlays(), { immediate: true })
  const overlayTimer = window.setInterval(() => void loadOverlays(), OVERLAYS_MS)
  const services = useServices()
  const checks = computed(() => preStreamChecks(twitchOverview.value, source.live(), source.modules(), overlays.value, services.unused))

  onScopeDispose(() => {
    window.clearInterval(clock)
    window.clearInterval(refresh)
    window.clearInterval(overlayTimer)
  })

  // 直播中 shows the live session; 下播後 the last ended one (a live session has no summary yet).
  const session = computed(() => {
    const s = summary.value
    if (!s) return undefined
    return (shown.value === 'live') === !s.endedAt ? s : undefined
  })
  // How long a live session has run, or when an ended one ran.
  const when = computed(() => {
    const s = session.value
    if (!s) return ''
    return s.endedAt ? sessionRange(s.startedAt, s.endedAt) : clockText(now.value - Date.parse(s.startedAt))
  })
  const line = computed(() => {
    const s = session.value
    if (!s) return ''
    const peak = s.peakViewers == null ? '' : ` · ${t('stage.peak', { n: s.peakViewers })}`
    return when.value + peak
  })
  const stats = computed(() => {
    const s = session.value
    if (!s) return []
    return [
      { key: 'follows', value: s.follows },
      { key: 'subs', value: s.subs },
      { key: 'bits', value: s.bits },
      { key: 'raids', value: s.raids },
      { key: 'redemptions', value: s.redemptions },
      { key: 'chatMessages', value: s.chatMessages },
      { key: 'firstChats', value: s.firstChats },
    ]
  })


  const exporting = ref(false)
  const exportNotice = ref<{ text: string; error: boolean }>()
  async function exportIt() {
    exporting.value = true
    exportNotice.value = undefined
    try {
      if (await exportRecord(session.value!.startedAt)) exportNotice.value = { text: t('record.exported'), error: false }
    } catch (e) {
      exportNotice.value = { text: e instanceof ApiError ? errorMessage(e.code) : String(e), error: true }
    } finally {
      exporting.value = false
    }
  }

  // 直播中 picked while Twitch says the stream is not live.
  const notLive = computed(() => shown.value === 'live' && source.live().stream?.state !== 'live')

  return { shown, manual, backToAuto, failed, notLive, checks, session, when, line, stats, exporting, exportNotice, exportIt }
}

export type StageState = ReturnType<typeof useStage>
