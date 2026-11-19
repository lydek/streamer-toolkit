<template>
  <!-- One chat message as the admin UI shows it (chat log page, docked panel); the display settings decide what shows.
       Viewer input only ever goes in as text ({{ }}) or as an attribute, never as markup. -->
  <div class="line" :class="[`style-${display.style}`, { highlighted: p.highlighted }]">
    <time v-if="display.time" :datetime="message.occurredAt">{{ clockText(message.occurredAt) }}</time>
    <img v-if="display.avatars && avatar" class="avatar" :src="avatar" alt="" />
    <template v-if="display.badges">
      <img v-for="b in badges" :key="`${b.setId}/${b.id}`" class="badge-img" :src="b.url" :alt="b.title" :title="b.title" />
    </template>
    <span v-if="ownBadge" class="badge">{{ t('chat.broadcaster') }}</span>
    <span class="name" :style="nameColor ? { color: nameColor } : undefined">{{ p.userName }}</span>
    <span v-if="p.sharedFrom" class="tag">{{ t('chat.sharedFrom', { channel: p.sharedFrom.userName }) }}</span>
    <span v-if="p.reply" class="tag">{{ t('chat.replyTo', { name: p.reply.userName }) }}</span>
    <span v-if="p.bits" class="tag bits">{{ t('chat.bits', { bits: p.bits }) }}</span>：<span
      v-if="message.deleted"
      class="deleted"
      >{{ t('chat.deleted') }}</span
    ><template v-else>{{ p.text }}</template>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { avatars, badgeImages, chatDisplay as display } from './chatDisplay'
import type { LoggedChat } from './chatLog'
import { clockText } from './chatLogView'

const props = defineProps<{ message: LoggedChat; nameColor?: string; broadcasterId?: string }>()
const { t } = useI18n()

const p = computed(() => props.message.payload)
const avatar = computed(() => avatars.value.get(p.value.userId))
// Only badges whose image Twitch gave us (from its CDN); the rest show nothing.
const badges = computed(() =>
  (p.value.twitchBadges ?? []).flatMap((b) => {
    const image = badgeImages.value.get(`${b.setId}/${b.id}`)
    return image ? [image] : []
  }),
)
// The broadcaster's own lines are marked in words when no badge image says so.
const ownBadge = computed(
  () => !!props.broadcasterId && p.value.userId === props.broadcasterId && !badges.value.some((b) => b.setId === 'broadcaster'),
)
</script>

<style scoped>
.line {
  overflow-wrap: anywhere;
  line-height: 1.5;
}

.style-compact {
  line-height: 1.3;
}

.style-comfortable {
  padding: 0.25rem 0;
}

.highlighted {
  border-left: 3px solid var(--p-primary-color);
  padding-left: 0.375rem;
}

time {
  margin-right: 0.375rem;
  color: var(--p-text-muted-color);
  font-variant-numeric: tabular-nums;
  font-size: 0.875em;
}

.avatar,
.badge-img {
  width: 1.125em;
  height: 1.125em;
  margin-right: 0.25rem;
  vertical-align: -0.2em;
}

.avatar {
  border-radius: 50%;
}

.style-comfortable .avatar {
  width: 1.5em;
  height: 1.5em;
  vertical-align: -0.4em;
}

.name {
  font-weight: 600;
}

/* surface.950 on primary.400: 11:1. */
.badge {
  margin-inline-end: 0.25rem;
  padding: 0 0.375rem;
  border-radius: 6px;
  background: var(--p-primary-400);
  color: var(--p-surface-950);
  font-size: 0.875em;
}

.tag {
  margin-left: 0.375rem;
  padding: 0 0.375rem;
  border-radius: 999px;
  background: var(--p-surface-800);
  color: var(--p-text-muted-color);
  font-size: 0.75em;
}

.bits {
  color: var(--p-yellow-400);
}

.deleted {
  color: var(--p-text-muted-color);
  font-style: italic;
}
</style>
