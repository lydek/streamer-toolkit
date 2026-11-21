import { i18n } from '../i18n'
import { type Schemas, api, client } from './api'
import { saveFile } from './save'

type Record = Schemas['StreamRecord']
type Row = Schemas['RecordRow']

/** Marks the file as UTF-8 for Excel. */
const BOM = String.fromCharCode(0xfeff)

/** A spreadsheet reads a cell starting with one of these as a formula. */
const FORMULA_START = /^[=+\-@\t\r\n]/

/** One quoted CSV cell; text a spreadsheet would run as a formula gets a leading `'`, so it stays text. */
export function csvCell(text: string): string {
  const safe = FORMULA_START.test(text) ? `'${text}` : text
  return `"${safe.replaceAll('"', '""')}"`
}

const pad = (n: number) => String(n).padStart(2, '0')
const localTime = (d: Date) =>
  `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`

/** The amount or content column: a sub's tier, a resub's months and message, bits, raiders, a reward and input. */
function detail(r: Row): string {
  const { t } = i18n.global
  const withText = (head: string, text: string | null) => (text ? t('record.withText', { head, text }) : head)
  switch (r.kind) {
    case 'follow':
      return ''
    case 'subscribe':
      return t(r.gifted ? 'record.gifted' : 'record.tier', { tier: r.tier ?? 1 })
    case 'resubscribe':
      return withText(t('record.months', { n: r.months ?? 0 }), r.message)
    case 'cheer':
      return withText(t('record.bits', { n: r.bits ?? 0 }), r.message)
    case 'raid':
      return t('record.raiders', { n: r.viewers ?? 0 })
    case 'redemption':
      return withText(r.reward ?? '', r.input)
  }
}

/** The record as CSV: UTF-8 with a BOM (so Excel reads the Chinese right), CRLF lines, every cell quoted. */
export function recordCsv(record: Record): string {
  const { t } = i18n.global
  const header = [t('record.at'), t('record.kind'), t('record.viewer'), t('record.detail')]
  const rows = record.rows.map((r) => [
    localTime(new Date(r.at)),
    t(`record.kinds.${r.kind}`),
    r.viewer ?? t('alerts.anonymous'),
    detail(r),
  ])
  return `${BOM}${[header, ...rows].map((cells) => cells.map(csvCell).join(',')).join('\r\n')}\r\n`
}

/** e.g. `streamer-toolkit-record-20261121-1800.csv`, from the stream's start in local time. */
export function recordFileName(startedAt: string): string {
  const d = new Date(startedAt)
  return `streamer-toolkit-record-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}.csv`
}

/**
 * Asks where to save (before anything else, while the click still allows a save dialog), then writes the latest
 * stream's record there. Resolves `false` when cancelled.
 */
export const exportRecord = (startedAt: string) =>
  saveFile(recordFileName(startedAt), { description: 'CSV', mime: 'text/csv', extension: '.csv' }, async () => {
    const record = await api(client.GET('/api/admin/stream/record'))
    if (!record) throw new Error('no stream')
    return new Blob([recordCsv(record)], { type: 'text/csv;charset=utf-8' })
  })
