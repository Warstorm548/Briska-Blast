# Update progress & post-update relaunch

Shipped in **launcher v0.22.0**. Three connected pieces:

1. An **update plan** — the ordered, weighted step list one update job is made of.
2. The **progress display** that renders it.
3. The **relaunch handshake** that brings the launcher back after it updates itself.

The plan is deliberately over-general for today's needs. It is the groundwork for
phased updates (several stages applied in order to reach a target version) and for
modular component updates once the game is large enough to split up.

---

## 1. The update plan (`launcher/src/updater/plan.rs`)

An `UpdatePlan` is an ordered `Vec<Step>`. Each step is a `Phase`
(`Downloading` / `Installing` / `Verifying`) applied to one component, carrying a
`weight` in cost units.

Two numbers come out of a plan, and they deliberately disagree:

| Output | Meaning | Resets per stage? |
|---|---|---|
| `label(index, fraction)` | The text. Phase, step fraction, percent **within that step**. | Yes |
| `overall_fraction(index, fraction)` | The bar. One weighted position across every step. | No |

The text says where you are in the current piece of work. The bar says how much of
everything is left. A user watching a download finish sees the percentage restart
at zero for the install stage while the bar stays where it was.

**Step counts are a property of the job.** A game update builds three steps; a
launcher self-update builds two, because a single executable has no `files.json` to
verify against. Nothing anywhere hardcodes "2" or "3".

### Label grammar

```
<Phase> <step>/<steps>  [<component> <n>/<count>]  <pct>%
```

```
Downloading 1/3  47%                    one component (today)
Downloading 1/3  audio 3/10  47%        many components (later)
```

The component fraction is **omitted while there is only one component**, so today's
text carries no `1/1` noise.

Both fractions are kept because they answer different questions. The step fraction
says how many *kinds* of work remain; the component fraction says how many pieces of
the current kind remain. A ten-component download would otherwise sit at `1/3` for
its whole duration and look frozen.

When components arrive, the expected order is **phase-major**: download every
component, then install every one, then verify every one. Nothing is swapped into
the live install until every piece has arrived and been checked.

### Weighting

`weight = bytes × Phase::cost_per_byte()`.

Raw bytes alone would misweight the bar, because a megabyte over the network and a
megabyte through sha256 are not the same amount of waiting. The coefficients in
`Phase::cost_per_byte` are the one place to tune bar smoothness. Relative sizes
*within* a phase always come from real manifest bytes and are unaffected by them.

Degenerate totals (a zero-size asset, or a `releases-cache.json` predating the
`size` field) fall back to evenly-weighted steps rather than dividing by zero.

There is deliberately **no mid-job reweighting**. Revising a completed step's weight
would move the bar backwards, which `overall_fraction_is_monotonic` forbids.

---

## 2. Where the numbers come from

| Step | Source | Cost |
|---|---|---|
| Download | `size` on the release asset, from the GitHub releases API | free — already fetched and cached |
| Install | sum of sizes in the release's standalone `files.json` | one request, at install time only |
| Verify | same total as install | shares the fetch above |

`github_client::Asset::size` is `#[serde(default)]`, which is load-bearing: the field
is absent from every `releases-cache.json` written before it existed, and a cold
start must not have to re-fetch the whole list to learn sizes.

### The standalone manifest asset

`files.json` ships **inside** each archive, where `verify_install` reads it from —
but that is only available *after* the download it was meant to size. So
`release-client.yml` now also publishes it as its own asset:

```
briskablast-client-<channel>-<version>-<platform>-files.json
```

`installer::download::fetch_installed_bytes` reads it before the archive download
starts. Every failure mode — no such asset, fetch failed, unparseable, rate-limit
gate closed — returns `None` and falls back to
`plan::estimate_installed_bytes` (a fixed ratio of the compressed size). **None of
them may ever fail an install**; the worst outcome is a less evenly paced bar.

This asset is also what will declare the component list later, so components are
enumerated from release data rather than compiled into the launcher.

---

## 3. Progress events

The installer reports the **phase** it is in, not a step number:

```rust
InstallProgress::Phase { phase, fraction, bytes_now, bytes_total }
```

`UpdatePlan::index_of_phase` resolves phase to step, so step ordering is defined in
exactly one place. Having the installer count steps too would mean two places had to
agree. The self-update emits the same type, so one handler
(`handlers::install::apply_progress`) folds both into the bar.

**Extraction progress is measured from outside.** `tar.unpack` and `zip.extract` are
single opaque calls with no callback, and re-implementing them entry-by-entry would
put the macOS bundle's symlinks and exec bits — on which its ad-hoc signature
depends — at risk for a cosmetic gain. Instead a poller measures bytes landed in the
staging tree every 200 ms against the manifest's real total. The poller is `abort()`ed
**and awaited** before the next phase starts, so a late event cannot make the bar
jump backwards.

### Verify runs before the swap

The third step hashes **staging**, not the live install. A corrupt download therefore
fails through the caller's existing staging-cleanup path with the previous version
untouched, and no rollback machinery is needed. Verifying after the swap would only
report that something broken had already replaced something that worked.

A release with no `files.json` at all still installs: `verify_install` falls back to
its historic exe-exists check, exactly as Settings → Verify does.

---

## 4. The progress bar's appearance

`ui::theme::progress_track` gives the bar a **light** track and a slightly brighter
filled portion, and the status text is black and centred **on** the bar
(`ui::bottom_bar::progress_cell`, an Iced `stack`).

Both halves are light on purpose, in an otherwise dark UI. The label sits on the bar,
so the *unfilled* track has to carry black text as readably as the filled part does.
A dark track would swallow the label for as long as the bar was near empty — which is
when a user is most likely to be reading it.

---

## 5. Relaunch after self-update (`launcher/src/updater/relaunch.rs`)

A successful self-update replaces the launcher on disk, so the running process must
exit — it is executing code that no longer exists there. It now starts the
replacement first, detached, then exits.

| Platform | What is restarted |
|---|---|
| Windows | `current_exe()`, with `DETACHED_PROCESS \| CREATE_NEW_PROCESS_GROUP` |
| Linux (AppImage) | the outer `$APPIMAGE` file the swap replaced |
| Linux (bare binary) | `current_exe()` |
| Linux (`.deb`, system-wide) | never — self-update is refused there up front |
| macOS (`.app`) | `open -n <bundle>`, so Launch Services gives it proper app identity |
| macOS (bare binary) | `current_exe()` |

### The single-instance handshake

The launcher claims a slot by writing a discovery file naming a loopback port it
listens on; a launcher that finds a **live** listener exits as a duplicate
(`rendezvous.rs`). A child started while its parent is still alive would therefore
quit immediately — leaving the user with **nothing running at all**, which is worse
than the bug being fixed.

Rather than add a new coordination mechanism, the child is started with
`--after-update` and `main` then calls `rendezvous::acquire_launcher_waiting`, which
retries for up to ten seconds instead of conceding on the first refusal. The parent
exits within milliseconds; its listener dies with it; the child's next probe is
refused and the **existing** stale-file reclaim takes over.

This does not weaken single-instance: a genuine second instance outlives the timeout
and still loses. Covered by `waiting_acquire_still_concedes_to_a_live_instance`.

Waiting also fixes a Windows leftover: the rename trick leaves a
`.__relocated__.exe` that cannot be deleted while the old process holds it, and the
replacement only reaches `cleanup_stale_update_artifacts` once it holds the slot —
by which point the parent is gone.

### What a self-update does *not* touch

Everything the launcher remembers lives in the per-user data directory, never beside
the binary: `identity.json` (player ids + tokens), `ratelimits.json`,
`releases-cache.json`, `changelogs/`, `saves/`. The swap replaces only the executable,
the `.app` bundle, or the outer AppImage. Every one of those files is also written
temp-then-rename, so an abrupt exit can lose the newest write but never leave a torn
file.

### Binary swap

`updater::binary_swap` replaces `self_update`'s `Update::update()`, which did the same
work as one opaque blocking call with no progress callback. It is assembled from
`asset_fetch` (the streaming, rate-limit-aware download the macOS and Linux paths
already used) plus `self_replace` — the crate `self_update` re-exports and was already
delegating the swap to. The on-disk outcome is unchanged.

---

## Testing notes

`cargo build -p launcher --target x86_64-pc-windows-gnu` is **required** before
pushing: the detached spawn and the zip extraction are `cfg(windows)` code that a
Linux build never compiles.

The relaunch itself is only observable on a real install — see
`docs/planning/known-bugs.md` if it misbehaves in the field.
