# Launcher changelog viewer

Shipped in launcher **v0.21.0** (2026-09-10). Surfaces the game and launcher
changelogs inside the launcher, so users can review what changed without digging
through the repo, and can read what a pending update contains **before** starting
the download.

Code: `launcher/src/changelog/` (store, parser, fetch) and
`launcher/src/ui/center/changelog.rs` (the shared accordion widget).
State and message handling: `launcher/src/app/handlers/changelog.rs`.

---

## Where it appears

| Surface | Contents |
|---|---|
| Default center view | The focused channel's game changelog, anchored at that channel's **installed** version |
| Game install / update prompt | Every entry between installed and the version being installed, below the Cancel/Confirm row |
| Launcher Update view | Entries between the running launcher and the version on offer, below the button row |
| Settings → Launcher Changelog | The last 10 launcher releases at or below the running version |

All four render through one widget, so they cannot drift apart. Entries are
collapsed with the newest one expanded; each pane links out to the complete file
on GitHub.

The game changelog is the **default center view** rather than a menu: it is what
you see when no menu is open, so picking a channel while Settings or an install
prompt is open never yanks you out of what you were doing. A channel with nothing
installed has no anchor, so it keeps the original "No menu selected." placeholder.

---

## Where the text comes from

Three sources, in precedence order:

1. **Runtime refresh** — `raw.githubusercontent.com/Warstorm548/Briska-Blast/dev/<file>`,
   fetched once per launch with `If-None-Match`.
2. **Disk cache** — `<data_dir>/changelogs/` holds the refreshed `.md` files plus
   an `etags.json` sidecar. Stored as plain markdown so the cache is readable on
   disk.
3. **Bundled copy** — compiled in with `include_str!` from the repo root, so a
   first run with no network still has history.

On load the disk cache and the bundled copy are compared by **top version** and
the newer wins. That matters after a self-update: the launcher's freshly bundled
changelog can easily be newer than whatever its disk cache last recorded.

A refresh that fetches successfully but parses to zero entries is rejected rather
than cached — that shape is an error page or a truncated body, and overwriting a
good cache with it would be worse than staying stale.

### Rate limits

**These fetches do not touch the GitHub API rate limit.**
`raw.githubusercontent.com` is a plain file CDN, separate from `api.github.com`;
its responses carry no `x-ratelimit-*` headers at all. Verified by measurement:
three raw changelog fetches between two probes of the releases endpoint left
`x-ratelimit-used` incremented only by the probes themselves. So unlike
`updater/release_cache.rs`, this module is deliberately **not** behind
`crate::ratelimit`'s gate — gating it would make the changelog stale for no
benefit.

Note the contrast with the REST API, where a conditional `304` **does** still
cost a request for an unauthenticated caller. See
[`../planning/launcher-github-ratelimit-safety-net.md`](../planning/launcher-github-ratelimit-safety-net.md).

### Why the `dev` branch

`dev` is the branch that gets committed to first, so it is where changelog text
lands earliest. The consequence is that a Stable user reads the dev branch's copy
of the file — which is why the channel filter below exists, and why an entry
edited on `dev` after a stable release will show its edited text.

---

## Anchoring and filtering

**Anchor.** The channel pane lists entries at or **below** the installed version:
that entry on top, the nine before it, nothing newer. The pane reads as "what you
have", not "what exists"; anything newer is what the update prompt is for.

**Range.** The update prompt lists entries strictly newer than installed, up to
and including the target, capped at 10 so a long skip stays readable. A fresh
install with no prior version shows only the target's entry.

**Base versions.** Changelog headings are plain `MAJOR.MINOR.PATCH`, while an
installed version carries its channel suffix (`0.35.1-dev.1`). Both sides are
compared after stripping the prerelease, which is what lets them meet.

**Channel filter.** One changelog file covers every channel and its headings carry
no channel marker — but the release tags do. The set of versions actually shipped
to a channel is derived from the shared release list
(`game-v0.35.1-dev.1` → dev shipped `0.35.1`), and entries outside that set are
hidden. So a Stable user never reads about a version that only ever existed on
dev. This costs no request: it reads the snapshot `release_cache` already holds.

**"Not derived yet" and "derived, and this channel has nothing" are different
states, and must stay that way.** Collapsing them into "no filter" would render
the file *unfiltered*, which means showing a Stable user entries that only ever
reached dev. That is a live case, not a hypothetical: the repo currently has
dev-only game releases, so Stable's shipped set is legitimately empty.

- Filter not derived yet, or its derivation failed → the channel pane withholds
  entries and says so, and the update prompt falls back to the release body
  (which is authoritative for exactly the version being installed, so it carries
  no channel ambiguity).
- Filter derived → apply it. An **empty** set correctly hides every entry.

### Links are restricted to http/https

A link inside a rendered entry is markdown the launcher fetched from the network,
so it is data the launcher did not author. Every URL — both in-entry links and the
"full changelog" button — is parsed and rejected unless its scheme is exactly
`http` or `https`, because `open::that` hands anything else to the OS shell, where
`file://`, a UNC path, or a registered protocol handler can launch rather than
browse.

---

## Release bodies (CI)

Both release workflows slice the matching section out of the changelog and publish
it as the release body (`body_path`), replacing what was there before: **nothing
at all** for launcher releases, and an auto-generated commit list for game
releases.

- The release job now runs `actions/checkout` **before** `download-artifact` —
  checkout defaults to `clean: true` and would otherwise wipe the artifacts.
- The client reuses `needs.build.outputs.version` (derived from
  `client/project.godot`, which already matches the changelog headings). The
  launcher re-derives it from the tag, stripping the `-ea.N` / `-dev.N` suffix,
  because its build jobs declare no job-level `outputs:`.
- A missing section is a warning, never a failure: the release publishes with an
  empty body, exactly as it did before.
- `generate_release_notes: true` stays on for the client; the generated notes are
  appended *below* an explicit body, so the curated changelog leads.

The launcher uses the release body as its **fallback** in the update prompt when
its local changelog has no section for the target version.

---

## Parser notes

`launcher/src/changelog/parse.rs` splits on `## [<semver>] — <date>`.

Two details that silently drop every entry if you get them wrong:

- The separator is an **em dash (U+2014)**, not a hyphen. The parser also accepts
  an en dash or hyphen, so a future typo costs a missing date rather than a
  missing entry.
- Each body is terminated by a `---` rule that belongs to the file's layout, not
  to the entry, and both files open with a preamble before the first heading.
  Both are stripped.

Headings that are not valid semver (`## [Unreleased]`) are ignored rather than
producing a bogus entry. Entries are sorted by parsed semver rather than trusting
file order.

A test parses the **bundled** files on every build and asserts the launcher's own
crate version has an entry, so a release that bumps `Cargo.toml` without writing a
changelog section fails the test suite.
