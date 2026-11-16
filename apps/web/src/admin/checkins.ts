import type { Incoming } from '../overlay/chat/chat'

export const PAGE_SIZE = 50

/** Query of `GET /api/admin/checkin/viewers`. */
export const viewerQuery = (search: string, page = 0, pageSize = PAGE_SIZE) => ({
  search: search.trim() || undefined,
  offset: page * pageSize,
  limit: pageSize,
})

/** Whether a `/ws/overlay` message means check-ins may have changed; a snapshot counts (changes made while
 *  disconnected are not replayed). */
export const changesCheckins = (msg: Incoming) => msg.type === 'checkin.changed' || msg.type === 'snapshot'

/** The local date (`YYYY-MM-DD`), the most a back-fill can pick; the Host checks the business day itself. */
export function localDate(d = new Date()) {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}
