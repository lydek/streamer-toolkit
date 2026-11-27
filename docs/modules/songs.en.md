<!-- source: docs/modules/songs.md source-sha256: bd5fa7ca5e6013378a4fb918258416df9dcfe82c8532a6d7752766f52c5073d4 -->
[繁體中文](songs.md) | **English**

# Song requests

A complex module (see [architecture.en.md](../architecture.en.md) §3): a song is stored only once, requests have illegal state transitions, the rules (per-viewer limits, the same song's cooldown) must hold the same way for every way of requesting, and songs requested with channel points involve refunds, so mistakes hurt viewers' trust directly. This file covers only rules, state transitions and acceptance.

Module `songs` (the feature list's song requests: the requests, song list and request rules pages), on by default (it needs no Twitch scope).

## Song list
- A song: title (1–100 characters), artist (optional, up to 100), category (optional; none is "uncategorised"), YouTube link (optional; only `youtube.com/watch?v=` and `youtu.be/` addresses), when it was last sung, and how many times it was requested.
- The same song means the same title and artist once normalised (leading and trailing blanks trimmed, runs of blanks as one, full-width and half-width and letter case treated alike); a database unique constraint blocks duplicates too, not only the code.
- Categories: a name of 1–30 characters, unique ignoring case; each can be open to requests or not. Deleting a category moves its songs to uncategorised; none are deleted. Uncategorised songs can be requested.
- At most 5,000 songs and 100 categories.
- Internal IDs are UUID v7; times are stored in UTC.

## Import
- Pick an Excel (`.xlsx`) or CSV (UTF-8, with or without a BOM) file on the song list page, up to 5 MB: the admin UI sends it to the Host for a preview (how many songs are added, updated and skipped, with the reasons), and only after confirming is it written in one transaction. The source file is never changed.
- The first row is the header, and columns are found by it: title (`歌名`, `title`, `song`; required), artist (`歌手`, `artist`, `singer`), category (`分類`, `category`) and link (`YouTube`, `連結`, `link`, `url`), ignoring case; other columns are ignored. Every sheet of an Excel file is read; without a category column, the sheet's name is the category.
- A song already there (the same song) is updated: the category and link filled in the file replace the stored ones, and blank cells never overwrite stored values. New categories are created along the way, open to requests by default.
- Skipped, with the reasons listed: no title, a title or artist too long, a link that is not a YouTube address, the same song as an earlier row of the file, or past the song list's limit.
- Importing the same file twice gives the same result.

## Requesting
- A viewer types `!點歌 text` in chat: the command is named 點歌 by default and shares the command table with other features (`songs.request`), so a name belongs to only one feature; who may use it follows the four shared levels.
- Matching: the text, normalised, equals a title exactly, or is "title artist" or "artist title" (matching typos is P4-3). It is queued only when exactly one song matches; when several share the title, the viewer is asked to add the artist.
- Only songs in a category open to requests (or uncategorised) can be requested.
- The same song is never queued or singing twice at once.
- At most 100 songs wait in the queue.
- Each time a song is queued, its request count goes up by 1.
- The streamer can request a song by hand on the requests page, optionally for a viewer; requests by hand ignore whether the category is open.
- Simulated events (`source=simulation`) and rehearsals never queue anything; the bot's own messages trigger nothing.
- With the module off, requests from chat are ignored; songs already queued stay.

## State transitions
| From | Can become |
|---|---|
| Queued (`queued`) | Singing (`singing`), removed (`removed`) |
| Singing | Sung (`sung`), removed |
| Sung | Final |
| Removed | Final |

- At most one song is singing at a time. 唱完了 marks the singing song sung, updates that song's last-sung time, then starts the first queued song; with nothing singing, any queued song can be started.
- Illegal transitions are refused, and the record stays as it was.
- The queue can be reordered by dragging; removing or starting a song leaves the others' order as it was.
- The sung list shows the latest sung first.

## Replies
- While the chat bot is on, it replies to each request with the result: queued (at which place), not found, several with that title, not open to requests, already queued, or the queue full. The texts are changed on the request rules page and may use `{觀眾}`, `{歌名}`, `{歌手}`, `{順位}` and `{輸入}`, fixed replacements only.
- Replies go through the chat bot's queue and follow its posting rules (it never triggers itself, keeps to the rate limit, sends nothing for simulated events); with the chat bot off nothing is replied, and requests are still queued.
- What viewers typed and song titles are shown and sent as text only, never read as variables.

## Data
- Songs, categories and requests are kept in the module's own tables; the reply texts under `songs` in `settings.json`; the command name and who may use it in the shared command table.

## Acceptance
- A song is stored only once (the database blocks it too); deleting a category deletes no songs; the import's preview, deduplication and skip reasons are visible, blank cells never overwrite stored values, and importing the same file twice gives the same result.
- Requests are queued by matching, open categories, no duplicates and the limit; requests by hand ignore open categories; request counts go up per request.
- States change only as in the table above; at most one song is singing; 唱完了 updates the last-sung time.
- Simulated events, rehearsals and the bot's own messages never queue anything; with the chat bot off nothing is replied but requests are still queued.
- All of the above have tests (the repository on real SQLite).
