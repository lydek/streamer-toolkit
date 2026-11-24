import { nextTick } from 'vue'
import type { RouteLocationNormalized } from 'vue-router'
import { focusTarget } from './drawer'

/*
 * Full pages at /admin/<feature> (docs/decisions.md 管理 UI 版面). FullPage.vue is the layout; a page is a route
 * whose component wraps its content in it:
 *
 *   <FullPage tile="…"> <template #actions>…</template> <section>…</section> </FullPage>
 *
 * - The title and icon are the page tab's; a page outside the tabs (settings) passes `title`. There is no back
 *   button: home is the 首頁 tab or Alt+←.
 * - Entering focuses the header's first action, else the title (or the element the URL hash names). Settings use
 *   `useDrawerSetting`; leaving the page submits them first, and a failed one keeps the page with its error shown
 *   under the title.
 * - Back on home, focus returns to the element that opened the page: a home element with `data-drawer-tile`,
 *   `data-page-opener` or `data-drawer-arrange` (a drawer's "Open full page" counts as its tile, because
 *   closing the drawer focuses the tile first), else the first tile, then "Arrange tiles".
 */

const OPENER_ATTRS = ['data-drawer-tile', 'data-page-opener', 'data-drawer-arrange']

/**
 * The page a category or page tab just opened: that page leaves focus on the category (still there) or moves it to
 * its own page tab (the old one went with the old page), instead of focusing the header.
 */
export const navOpened = { path: '' }

// The opener to focus once home has rendered again; undefined when home was not left for a page.
let returnTo: string | null | undefined

/** A selector that finds `el` again after home re-renders, or null when it is not a known opener. */
export function openerSelector(el: Element | null): string | null {
  for (const attr of OPENER_ATTRS) {
    const value = el?.getAttribute(attr)
    if (value != null) return `[${attr}="${value.replace(/["\\]/g, '\\$&')}"]`
  }
  return null
}

/**
 * Called by the router guard after the drawer closed: remembers the opener when home is left for a page.
 * `focused` is what had focus before the drawer closed (⚙ beside an open drawer); when it is no opener (a
 * drawer's "Open full page"), closing the drawer has moved focus to the drawer's tile.
 */
export function rememberOpener(to: RouteLocationNormalized, from: RouteLocationNormalized, focused: Element | null) {
  if (from.matched.length && from.path === '/' && to.path !== '/')
    returnTo = openerSelector(focused) ?? openerSelector(document.activeElement)
}

/** Called by home once its tiles have loaded: focuses the opener of the page the user came back from. */
export async function focusReturnTarget() {
  const selector = returnTo
  returnTo = undefined
  if (selector === undefined) return
  await nextTick()
  focusTarget(document, '', selector ? document.querySelector(selector) : null)?.focus()
}

/**
 * After an action whose focused control went away (a deleted row, a dismissed notice, a replaced form), moves
 * focus to `target` once the DOM has updated. Focus left elsewhere (the control is still there) stays.
 */
export async function keepFocus(target: () => HTMLElement | null | undefined) {
  await nextTick()
  if (!document.activeElement || document.activeElement === document.body) target()?.focus()
}
