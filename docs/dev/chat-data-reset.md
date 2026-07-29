# Chat Moderation Data Reset

How to wipe all chat-moderation state — transcripts, audit records, bans,
blacklist — and let it rebuild from empty, **without touching player identity or
live game sessions**.

Use this to start fresh after a period of testing, or when accumulated dev data
stops resembling anything real.

---

## Why this needs no code

Every key the chat-moderation system writes lives under the `chat:` prefix.
Everything you want to keep lives under a different one:

| Prefix | Holds | Reset? |
|---|---|---|
| `chat:` | transcripts, audit records, bans, blacklist, id sequences | **wiped** |
| `player:` | player ids, the freelist, usernames, tokens, dev flags | kept |
| `session:` | live game sessions | kept |
| `admin:` | admin password hash, rotation flag, admin sessions | kept |

So the reset is one prefix, with nothing to pick around.

It also rebuilds cleanly on its own. The 0.34.0 audit migration keys off the
pre-0.34.0 lists (`chat:audit:player` and friends) and per-category cursors
(`chat:audit:migrated:*`). A `chat:*` wipe removes **both together**, so the next
boot finds empty legacy lists and imports nothing.

> The one way to get this wrong is deleting the cursors while leaving the legacy
> lists in place — that re-imports every legacy record as a duplicate row. The
> command below cannot do that, because it removes both in one pass. See
> `chat::audit::migrate` for the full failure table.

---

## The commands

Run from the repository root, against the environment you want to reset.

```bash
# Wipe chat state, keeping the two id sequences (see below).
docker compose exec redis redis-cli --scan --pattern 'chat:*' \
  | grep -vE '^chat:(body|audit):(counter|epoch)$' \
  | xargs -r docker compose exec -T redis redis-cli DEL

# Verify: only the four counter/epoch keys should remain.
docker compose exec redis redis-cli --scan --pattern 'chat:*'
```

`--scan` rather than `KEYS` so a large keyspace doesn't block Redis.

**No restart is required.** Nothing in `AppState` caches chat data — every page
reads it from Redis on request.

---

## Run it with no live game sessions

`chat:live:{code}` maps a running session to its transcript instance. Wiping it
mid-game detaches active sessions from their chat history: the session keeps
working, but the moderation panel loses the thread it was following.

The `session:*` records themselves are untouched in either case — it is only the
chat side of a live session that is affected.

---

## Why the id counters are kept

`chat:body:counter` / `chat:body:epoch` and `chat:audit:counter` /
`chat:audit:epoch` are excluded from the wipe on purpose.

Resetting them *would* be safe here, because a total wipe destroys everything
that references those ids in the same command — the "never flush these" rule in
`chat::ids` exists to stop an id being reissued while records still point at the
old one, and after a full wipe no such record survives.

They are kept anyway because they are four integers and leaving them costs
nothing, while keeping the guarantee unconditional. It matters if a backup is
ever restored alongside live data: counters that never reset cannot collide with
anything, reset ones can.

To restart ids from `a00000000001`, drop the `grep -v` from the command above.
Only do this on an environment you are certain will never have older data
restored into it.

---

## What gets destroyed

Everything below is **permanently** lost — there is no soft-delete and no
archive:

- every retained transcript and its deletion marks
- every audit record in all four logs, including pinned ban evidence
- the ban list (**every banned player is un-banned**)
- the word blacklist
- flagged-session and retained-session bookkeeping

Bans in particular are worth pausing on: after the reset, previously banned
players can chat again immediately, and the records explaining why they were
banned are gone. On a real deployment, export or screenshot what you need first.

---

## Scope

This resets **chat moderation only**. It does not touch:

- player ids, usernames, secret tokens, or the `player:freelist` reuse pool
- live game sessions
- admin credentials, the password-rotation flag, or signed-in admin sessions
- runtime config such as `min_launcher_version` / `min_game_version`

---

## Verifying afterwards

1. `redis-cli --scan --pattern 'chat:*'` returns only the four counter keys
   (or nothing, if you chose to reset those too).
2. **Chat Audit Logs** shows the empty state for all four tables, naming the
   window it searched.
3. **Moderation Lists → Banned Users** and **Backlisted Words** are empty.
4. A new match produces a fresh transcript, and a Warn or Ban writes a record
   that appears in the tables it belongs to.

If anything unexpected appears under `chat:*` after the wipe, the key list in
this document has drifted from the code — the source of truth is the key
literals in `server/src/chat/`.
