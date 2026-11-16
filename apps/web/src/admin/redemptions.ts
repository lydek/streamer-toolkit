import type { Incoming } from '../overlay/chat/chat'
import type { Schemas } from './api'

export type RedemptionRecord = Schemas['RedemptionRecord']
export type RedemptionStatus = Schemas['RedemptionStatusName']

export const STATUSES: RedemptionStatus[] = ['unfulfilled', 'fulfilled', 'canceled', 'unknown']
export const PAGE_SIZE = 50
/** Records the drawer shows. */
export const RECENT = 5

/** Full-page filters as the form holds them; `from` and `to` are local dates (`YYYY-MM-DD`) or empty. */
export interface Filters {
  from: string
  to: string
  viewer: string
  reward: string
  status: RedemptionStatus | null
}

export const noFilters = (): Filters => ({ from: '', to: '', viewer: '', reward: '', status: null })

/** Local midnight of `date` plus `days`, as a UTC instant. */
function localDay(date: string, days = 0) {
  const [y, m, d] = date.split('-').map(Number)
  return new Date(y, m - 1, d + days).toISOString()
}

/** Query of `GET /api/admin/redemptions`: local dates become instants, and `to` covers its whole day. */
export function redemptionQuery(f: Filters, page = 0, pageSize = PAGE_SIZE) {
  return {
    from: f.from ? localDay(f.from) : undefined,
    to: f.to ? localDay(f.to, 1) : undefined,
    viewer: f.viewer.trim() || undefined,
    reward: f.reward.trim() || undefined,
    status: f.status ?? undefined,
    offset: page * pageSize,
    limit: pageSize,
  }
}

/** Whether a `/ws/overlay` message means the records may have changed. A snapshot counts: changes made while
 *  disconnected are not replayed. */
export const changesRecords = (msg: Incoming) => msg.type === 'redemption.changed' || msg.type === 'snapshot'

export const pageCount = (total: number, pageSize = PAGE_SIZE) => Math.max(1, Math.ceil(total / pageSize))
