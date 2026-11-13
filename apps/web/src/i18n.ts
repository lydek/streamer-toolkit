import { createI18n } from 'vue-i18n'
import zhTW from './locales/zh-TW.json'

// Messages are precompiled by @intlify/unplugin-vue-i18n, so the strict CSP needs no 'unsafe-eval'.
export const i18n = createI18n({
  legacy: false,
  locale: 'zh-TW',
  fallbackLocale: 'zh-TW',
  messages: { 'zh-TW': zhTW },
})

/** Stable error codes from the Host and IPC commands map to `errors.<code>`. */
export function errorMessage(code: string): string {
  const { t, te } = i18n.global
  const key = `errors.${code}`
  return te(key) ? t(key) : t('errors.unknown', { code })
}
