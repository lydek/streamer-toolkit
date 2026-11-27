<!-- source: docs/modules/chat-bot.md source-sha256: 256986727fd2e93705becd9a1427ca255fd4190e064ecac7d78745c9ff696f3e -->
[繁體中文](chat-bot.md) | **English**

# Chat bot

A complex module (see [architecture.en.md](../architecture.en.md) §3): the bot must never trigger itself, simulated events must never post, and posting past Twitch's limit makes Twitch ignore the streamer's account for 1 hour, so mistakes hurt viewers' trust directly. This file covers only rules and acceptance.

Module `bot` (the feature list's chat bot: the commands, timed messages, and thanks and greetings pages), off by default; turning it on first asks for `user:write:chat`.

## Posting
- Posts as the streamer through Helix Send Chat Message (`user:write:chat`); a separate bot account comes in phase 9. The Host implements the sending port; the module knows nothing of Helix.
- Command replies, timed messages, and thanks and greetings all go into one local queue and are sent in order, rate-limited with the conservative value: at most 20 messages in any 30 seconds. Messages the streamer sends by hand from the send box are not queued or delayed, but count toward the same 30 seconds.
- The queue holds at most 20 messages; once it is full, new automatic messages are dropped. A message still waiting after 2 minutes is dropped too (e.g. a flood of follows during a raid), so no stale thanks go out. The number dropped goes to the log and shows in the running total since launch.
- Variables are replaced first, then leading and trailing blanks trimmed; a blank message is not sent, and one over 500 characters is cut to 500.
- A failed send (Twitch refused it, did not send it with `is_sent=false`, or the rate limit is in effect) is not retried and goes to the log (without the message text). Signed out or without `user:write:chat`, nothing is sent or queued.
- Twitch's `message_id` of every message sent is kept for 5 minutes (at most 200). When chat brings a message with one of these IDs, the bot ignores it: it triggers no command, does not count toward a timed message's chat lines, and triggers no greeting. The chat message may arrive before Twitch's answer: while a send still waits for its answer, a message from the streamer's account with the same text as the one being sent is ignored too. Messages the streamer types are handled as usual.
- Simulated events (`source=simulation`) and rehearsals never trigger a message and never reach the queue.
- With the module off, nothing is sent; messages still in the queue are dropped.

## Variables
Fixed replacements only, with no conditions or scripts. A variable it does not know stays as typed.
- `{觀眾}`: the display name of the viewer who triggered it.
- `{數量}`: the bits, or the raid's viewers; other events have no such variable.
- `{計數:名稱}`: the current value of the counter named 名稱 (read through the counter module's public interface); without such a counter it stays as typed.
- `{直播時長}`: how long this stream has run (`H:MM`); 未開播 (not live) when there is no stream.
- Phase 4's song requests add `{歌單網址}`.

## Commands
- A command: name, reply, who may use it, cooldown (0–3600 seconds, 0 for none) and enabled. At most 100, each reply up to 500 characters.
- Names share the one command table of every feature (`chat_commands` in `settings.json`): the same naming rules, a name belongs to only one feature (`commands.taken` otherwise), and the same matching tells whether a viewer typed a command.
- Who may use it follows the four shared levels (everyone, subscriber, VIP, moderator), each also letting in the levels above it; the streamer may always use it.
- Cooldown: per command, from its last reply; a command typed during it gets no reply and no notice. The cooldown is the same for everyone, the streamer included.
- A disabled command does not reply, but keeps its name, so no other feature can take it.
- Text after the command (arguments) does not change the reply.
- Replies are posted as the streamer, so a reply containing a command name shows in chat, but by the posting rules it never triggers a command again.

## Timed messages
- One: name, message, every how many minutes (5–120), at least how many chat lines in between (0–100) and enabled. At most 20.
- Timed only while live (by the stream status): the first interval counts from the stream's start; at the end of a stream the timer stops, and the next stream starts it again from its start.
- When one is due, the chat lines since its last send (or since the stream started, if not sent yet this stream) are counted; with fewer than set, this time is skipped until the next interval, with nothing saved up to send later. The count leaves out the bot's own messages and simulated events.
- Several due at once go into the queue in order.
- The list shows each one's next time; 未開播 (not live) when there is no stream.

## Thanks and greetings
- Each row can be switched and its message changed: new followers, first chat, return visits, subs and resubs, bits in three tiers (1–99, 100–999, 1000 and more), and raids.
- Each Twitch event is thanked at most once: the same event received again sends nothing (by its event ID).
- Subs and resubs leave out gifts: Twitch sends a separate sub event for every gifted sub (`is_gift=true`), and these are not thanked, as with alerts, or one gift of 20 would fill the queue. The gift itself (`channel.subscription.gift`) has no row in the prototype and is not thanked in this phase.
- The streamer's own messages trigger neither first chat nor return visits.
- First chat uses the existing judgement (Twitch's first-chat mark, or the toolkit seeing the viewer for the first time when the mark cannot be read); the mark may arrive after its message, and counts when it arrives. A first chat is not a return visit.
- Return visits:
  - Triggered when a returning viewer chats for the first time this stream and their last-seen time before this message is longer ago than the set days (1–365, 7 by default). The viewer directory reports the last-seen time before updating it, rather than writing first and reading back.
  - At most once per viewer per stream; kept in memory and started again with each new stream. Nothing triggers when there is no stream.
  - Two ways: 只提示我 (only tell me) tags that chat line in the chat log and the docked chat with 回訪 · N 天沒來 (back after N days) and sends nothing; 發訊息 (post) sends the set message.
- Thanks and greetings are sent when not live too (except return visits).
- The prototype's 你已經來了 {次數} 次 (you have been here N times) needs a count of the streams each viewer came to, which this phase does not keep, so `{次數}` is not offered.

## Data
- Commands (except their names and who may use them, which live in the command table), timed messages, and thanks and greetings are kept in `settings.json`; no table of its own, and no viewer data of its own.
- Which viewers this stream already got a return-visit greeting, and the kept `message_id`s, live in memory only and are cleared on restart.

## Acceptance
- Commands, variables, who may use them and cooldowns work as set; a disabled command does not reply; a command name never clashes with another feature's.
- Messages the bot sends never trigger a command, count toward a timed message's chat lines or trigger a greeting (even with a trigger word in them), also when the chat message arrives before Twitch's answer; commands the streamer types get replies as usual.
- Simulated events and rehearsals send nothing; nothing is sent with the module off, signed out, or without the scope.
- At most 20 messages go out in any 30 seconds (messages sent by hand included); messages dropped because the queue was full or they waited too long go to the log.
- Timed messages are sent only while live, at the set interval and chat-line minimum, skipped when chat was too quiet; none after the stream ends.
- Each kind of thanks and greeting is sent as set; the same event only once; bits pick the message by tier; sub events from gifts are not thanked; the streamer triggers neither first chat nor return visits.
- Return-visit days are counted from the last-seen time before this message; a second message in the same stream does not trigger it again; 只提示我 only tags and sends nothing.
- All of the above have tests (sending through a fake port, time from a controllable clock).
