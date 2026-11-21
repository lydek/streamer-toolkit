import { afterEach, describe, expect, it, vi } from 'vitest'
import { type Schemas, client } from './api'
import { csvCell, exportRecord, recordCsv, recordFileName } from './streamRecord'

type Row = Schemas['RecordRow']
const row = (p: Partial<Row>): Row => ({
  at: '2026-11-21T10:01:00.000Z',
  kind: 'follow',
  viewer: '阿凱',
  tier: null,
  gifted: false,
  months: null,
  bits: null,
  viewers: null,
  message: null,
  reward: null,
  input: null,
  ...p,
})
const record: Schemas['StreamRecord'] = {
  startedAt: '2026-11-21T10:00:00.000Z',
  endedAt: '2026-11-21T12:00:00.000Z',
  rows: [
    row({}),
    row({ kind: 'subscribe', tier: 1, gifted: true, viewer: '小魚' }),
    row({ kind: 'resubscribe', tier: 2, months: 6, message: '"嗨"' }),
    row({ kind: 'cheer', viewer: null, bits: 150 }),
    row({ kind: 'raid', viewer: 'nightowl', viewers: 12 }),
    row({ kind: 'redemption', viewer: '橘子', reward: '喝水', input: '多喝水' }),
  ],
}

/** The cells of each line, unquoted (the test data holds no commas or line breaks inside cells). */
const cells = (csv: string) =>
  csv
    .slice(1)
    .trimEnd()
    .split('\r\n')
    .map((line) => line.split('","').map((c) => c.replace(/^"|"$/g, '').replaceAll('""', '"')))

describe('stream record CSV', () => {
  it('starts with a BOM, uses CRLF lines and one row per event with its time, kind, viewer and detail', () => {
    const csv = recordCsv(record)
    expect(csv.charCodeAt(0)).toBe(0xfeff)
    expect(csv.endsWith('\r\n')).toBe(true)
    const lines = cells(csv)
    expect(lines[0]).toEqual(['時間', '事件種類', '觀眾', '數量或內容'])
    expect(lines.length).toBe(1 + record.rows.length)
    const local = new Date('2026-11-21T10:01:00.000Z')
    expect(lines[1][0]).toBe(
      `${local.getFullYear()}-11-${String(local.getDate()).padStart(2, '0')} ${String(local.getHours()).padStart(2, '0')}:01:00`,
    )
    expect(lines.slice(1).map((l) => l.slice(1))).toEqual([
      ['追隨', '阿凱', ''],
      ['訂閱', '小魚', '送禮訂閱 · Tier 1'],
      ['續訂', '阿凱', '6 個月："嗨"'],
      ['小奇點', '匿名觀眾', '150 小奇點'],
      ['揪團', 'nightowl', '12 人'],
      ['兌換', '橘子', '喝水：多喝水'],
    ])
  })

  it('never lets a spreadsheet run a cell as a formula', () => {
    for (const text of ['=HYPERLINK("x")', '+1', '-2', '@SUM(A1)', '\tx', '\rx', '\nx']) {
      expect(csvCell(text), JSON.stringify(text)).toBe(`"'${text.replaceAll('"', '""')}"`)
    }
    expect(csvCell('小魚')).toBe('"小魚"')
    expect(csvCell('a"b')).toBe('"a""b"')
    const csv = recordCsv({ ...record, rows: [row({ viewer: '=cmd|calc', kind: 'redemption', reward: '喝水', input: '@everyone' })] })
    expect(csv).toContain(`"'=cmd|calc"`)
    expect(csv).toContain('"喝水：@everyone"')
  })

  it('names the file after the stream start and saves it through the save dialog', async () => {
    const start = new Date('2026-11-21T10:00:00.000Z')
    expect(recordFileName(start.toISOString())).toBe(
      `streamer-toolkit-record-${start.getFullYear()}${String(start.getMonth() + 1).padStart(2, '0')}${String(start.getDate()).padStart(2, '0')}-${String(start.getHours()).padStart(2, '0')}00.csv`,
    )
    const written: Uint8Array[] = []
    const picker = vi.fn(async () => ({
      createWritable: async () => new WritableStream<Uint8Array>({ write: (c) => void written.push(c) }),
    }))
    vi.stubGlobal('showSaveFilePicker', picker)
    vi.spyOn(client, 'GET').mockImplementation((() =>
      Promise.resolve({ data: record, response: new Response(null, { status: 200 }) })) as never)
    expect(await exportRecord(record.startedAt)).toBe(true)
    expect(picker).toHaveBeenCalledWith(expect.objectContaining({ types: [{ description: 'CSV', accept: { 'text/csv': ['.csv'] } }] }))
    const text = new TextDecoder().decode(new Uint8Array(written.flatMap((c) => [...c])))
    expect(text).toContain('喝水：多喝水')
  })
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})
