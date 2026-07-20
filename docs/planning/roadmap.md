# Roadmap

Tracks deferred work and decisions made out of the current sprint.
For *how* an item should be built, follow the link to its spec doc.

---

## Post-Deployment Follow-Ups

Work intentionally deferred until after the initial production deployment of the server (v0.4.1).

### Bulk user deletion in the admin Users tab

- **What**: Multi-select / "delete all matching" on the Users tab, on top of the per-row delete shipped in server v0.12.0. Would batch the same `delete_user` work (wipe keys + `ZADD player:freelist`) across many ids in one confirm.
- **Why deferred**: Single-operator, small player set today — per-row delete is enough to prune stale accounts. Bulk delete needs a selection UI, a wider blast-radius confirm, and care around partial failures, for a workflow that isn't pressing yet.
- **Trigger to start**: Player count grows enough that pruning one row at a time is tedious, OR a spam/abuse cleanup needs to remove many ids at once.
- **Related**: the per-row handler (`server/src/admin/users.rs::delete_user`) and reuse pool (`player:freelist`) are already the seam — bulk is a loop over the existing primitive.

### Admin Logs tab (container output on the dedi)

- **What**: A `Logs` tab in the admin panel that reads container stdout/stderr straight from the host's Docker daemon and renders it in the browser, with a source selector (`server` / `redis` / `watchtower`), a line-count selector, client-side level + search filters, manual + auto-refresh, and a **Download .log** button for sending output to the developer. Reads via the already-present `bollard 0.17.1` (its `logs()` returns demuxed `StdOut`/`StdErr` frames) over the `/var/run/docker.sock` already mounted for the update flow — **no new dependencies or privileges**. Containers discovered by `com.docker.compose.service` label, scoped to this deployment's `com.docker.compose.project` so side-by-side channels never cross-read. Gated to `RELEASE_CHANNEL=dev` (nav link in `templates::nav_html` + routes in `main.rs`); the handler module stays channel-agnostic so opening it to ea/stable later is a one-line change. Slots into the same tab pattern Stats established (`server/src/admin/` module + route + `templates::*_page`), reusing `require_session` for auth.
- **Why deferred**: Design is settled but the user wants more time to think through scope before building — in particular whether to also do the durable side (a Redis audit-event stream that survives container recreation on update), live streaming follow (bollard `follow:true` via SSE/WS) vs the static tail-on-load MVP, and whether it should ultimately be all-channels rather than dev-only. Raw container logs reset whenever Watchtower recreates the container on update, so the "what's happening now" view (container logs) and the "what happened over time" view (audit stream) are genuinely two features; better to decide that boundary deliberately than ship half of it.
- **Trigger to start**: The user is ready to commit to the scope (MVP container-logs tab vs. also building the audit stream), OR a live debugging need on the dedi makes browser-readable logs pressing.
- **Related**: leverages the existing bollard usage in `server/src/update/docker.rs`; auth via `server/src/admin/mod.rs::require_session`; the durable-audit option overlaps with the **Score / replay / audit persistence** entry's keyspace conventions.

### Pocket ID admin SSO

- **What**: Replace the bcrypt password login at `/admin/login` with Pocket ID OIDC (passkey-based SSO).
- **Why deferred**: First deployment risk is already high; adding an external identity provider widens the failure surface. Current bcrypt + Redis session auth is adequate for a single operator. Migration is cheap later because the auth surface is small and isolated — only `server/src/admin/auth.rs` and one route change; the dashboard, update system, and version controls all just check `require_session`.
- **Trigger to start**: A second admin user needs access, OR the operator has time to stand up the Pocket ID instance as production infra.
- **Spec**: [`pocket-id-integration.md`](../server/pocket-id-integration.md)

### Per-gamemode host-configurable player range

- **What**: Extend the session-creation API so the host can send a `[min, max]` pair (instead of a single `player_count`) when the selected gamemode supports a host-configurable range. The server would still re-validate the pair against the gamemode's authoritative `[min_players, max_players]` bounds.
- **Why deferred**: The initial session-validation work uses a single value because no current gamemode benefits from a host-chosen range. Locking in a pair schema now would couple the request format to gamemodes that don't exist yet and force every gamemode to adopt the same shape even when it isn't meaningful. Easier to extend the schema once a real driving gamemode lands.
- **Trigger to start**: A new gamemode is added whose intended UX requires the host to pick a sub-range inside the gamemode's allowed bounds (e.g. a mode that legitimately supports anywhere from 3 to 6 players and the host wants to cap their lobby at 4).

### TURN relay for symmetric-NAT players

- **What**: Add a TURN server (Cloudflare TURN free tier, or self-hosted coturn) and issue short-lived TURN credentials to clients in `Identified` / `start_signaling` payloads. Update the test harness and (eventually) the Godot client to include the TURN URL in their `RTCPeerConnection` `iceServers` list. Symmetric-NAT players can then route through TURN instead of being kicked.
- **Why deferred**: Roughly 5–10% of consumer routers are symmetric NATs. Shipping without TURN means that minority cannot play, but adding TURN requires either a vendor account (Cloudflare API key + credential issuance endpoint) or self-hosted infrastructure (coturn container, firewall ports, abuse hardening). Neither is a one-evening change, and the symmetric-NAT player can still verify the server works with peers who are reachable. Better to ship the signaling backbone first and revisit once the audience is large enough to justify the operational cost.
- **Trigger to start**: User reports of "can't connect" that correlate with NAT type, OR the Godot client lands and we want to broaden compatibility before public beta.
- **Related**: see the symmetric-NAT entries in [`session-multiplayer-edge-cases.md`](../architecture/session-multiplayer-edge-cases.md).

### WS-ticket auth (stop sending `secret_token` over the WebSocket)

- **What**: The launcher / client currently sends its `secret_token` as cleartext in the first WS `Identify` frame. Replace that with a short-lived signed ticket: `/host` and `/join` REST responses include a `ws_ticket` (HMAC-signed `{player_id, session_code, expiry}`), and the client opens the WebSocket as `/ws/session/{code}?ticket=...`. The server verifies the signature + expiry on upgrade; the raw token never crosses the WS.
- **Why deferred**: Production deployment will use `wss://` (TLS) which already protects the token in transit. Tickets are defense in depth, but not load-bearing yet — and shipping ticket infra requires signing-key management, expiry handling, and a launcher-side change. Easier to land after the launcher exists and we can update both ends together.
- **Trigger to start**: Security audit before any non-TLS deployment path opens up, OR the Godot client lands (good moment to define this part of the wire shape once for both crates).
- **Related**: see the WS auth replay entry in [`session-multiplayer-edge-cases.md`](../architecture/session-multiplayer-edge-cases.md).

### Per-gamemode bounds in Redis (admin-editable)

- **What**: Move the hardcoded `[min_players, max_players]` table in `server/src/gamemode.rs::bounds_for` to Redis, seeded at first boot from compile-time defaults. Admin panel exposes per-gamemode editing. Same pattern the project already uses for `min_launcher_version`.
- **Why deferred**: Bounds are game-design constants today, not ops config. Promoting them to runtime config buys nothing until live balancing iteration becomes a real workflow, and adds Redis-key parsing/validation that doesn't exist yet (what if the stored value is `"abc"`?).
- **Trigger to start**: Active gamemode-balancing work where redeploying for each tweak is too slow, OR a third-party balance contributor needs to tweak bounds without source access.

### Score / replay / audit persistence

- **What**: Persist match outcomes — scores, winner, gamemode, duration, peer roster — to Redis keyed off `session_code` with an appropriate TTL. Establishes the keyspace conventions for any future server-side audit trail.
- **Why deferred**: No score logic exists yet; the Godot client isn't built; nothing is producing match outcomes.
- **Trigger to start**: Game client lands AND outcome tracking becomes a product requirement (recap screens, post-match stats, anti-cheat audit trail).

### Player profiles and cosmetics

- **What**: Extend the `player:<id>:*` Redis namespace beyond auth — store cosmetics, account metadata, persistent settings, unlock state.
- **Why deferred**: No non-auth per-player state exists. Adding the structure prematurely would lock in a schema before any feature drives it.
- **Trigger to start**: First non-auth player attribute lands (cosmetic unlock, persistent preference, account-bound stat).

### Leaderboards (Redis sorted sets)

- **What**: Per-gamemode leaderboards using Redis `ZADD` / `ZRANGE` / `ZREVRANGE`. Redis ZSETs are purpose-built for this access pattern — score-ordered, indexed, supports top-N and rank queries in O(log N).
- **Why deferred**: Requires score persistence (above) to exist first. No scoring mechanic in the game yet.
- **Trigger to start**: After Score / replay / audit persistence lands, when player-facing rankings become a product ask.

### Cross-instance rate limiting

- **What**: Replace the in-process `governor` rate limiters (currently `rl_register`, `rl_host`, `rl_join`, `rl_session`, `rl_admin_login` in `AppState`) with a Redis-backed implementation so multiple server instances enforce shared quotas instead of N independent per-instance ones.
- **Why deferred**: Single-instance deployment today — N independent limits IS one shared limit when N=1. Moving to Redis adds latency to every rate-limited path and a new failure mode (Redis blip → can't decide whether to rate-limit).
- **Trigger to start**: A second server instance comes online, OR a single attacker exhausts a single instance's quota in a way distributed enforcement would have caught.

### Multi-instance signaling presence

- **What**: Replace the per-process `SignalHub` (`server/src/signaling/mod.rs`) with Redis-backed presence (`SADD`/`SREM`/`SMEMBERS` per session) plus a pub/sub channel for cross-instance message routing. Sessions can then span instances — joiner A on server #1 and joiner B on server #2 can be in the same lobby.
- **Why deferred**: Documented in [`session-multiplayer-edge-cases.md`](../architecture/session-multiplayer-edge-cases.md) and the v0.5.0 plan as an explicit non-goal: signaling state is ephemeral, a restart drops every WS anyway, and the per-process map handles a single-instance deployment cleanly. Adding Redis writes/subs, cross-instance message routing, reconciliation between local and remote presence, and new failure modes is multi-hundred-line architectural work for capability not currently needed.
- **Trigger to start**: Concurrent session count exceeds what one instance can handle, OR HA failover (two instances, one passive) becomes a requirement.

### Installer-aware self-update (OS metadata sync)

- **What**: Replace or augment the `self_update` rename-trick swap (`launcher/src/updater/github.rs`) with a flow that also refreshes OS-level install metadata — Windows Add/Remove Programs `DisplayVersion` / `DisplayIcon`, the dpkg-recorded `launcher` package version on the `.deb` install path, and the NSIS-generated `Uninstall.exe` itself. Today self_update only swaps the one .exe file, so ARP keeps showing whatever the last `setup.exe` run wrote (e.g. ARP says `0.3.0-dev.5` even after a self-update to `0.3.1-dev.1`). Functionally harmless — the running binary's UI shows the real version — but cosmetically confusing for users who check Settings → Apps.
- **Why deferred**: The drift is purely cosmetic during dev/EA channels. Real security and protocol behavior come from the running binary, not the registry. Doing this right is multi-day work (Windows-specific UAC + signing concerns, Linux-specific dpkg-postinst awkwardness) for a polish issue, and bundles naturally with the code-signing pass — once `setup.exe` is signed by a trusted publisher (Azure Trusted Signing / OV / EV cert), a UAC-elevated full-installer re-run is no longer hostile UX. Trying to fix the cosmetic drift before signing trades one papercut for a worse one.
- **Trigger to start**: Code-signing pass lands, OR a public **stable** channel release approaches — whichever comes first. Both make the drift visible to end users rather than just devs.
- **Brainstorm — viable shapes** (pick at trigger time, do not pre-commit):
  - **(A) Silent reinstall via setup.exe.** Launcher downloads the full `BriskaBlast-Launcher-Setup-<ver>.exe` from the GitHub Release, runs it with `/S` (NSIS silent flag), exits. NSIS rewrites every OS metadata field correctly. Pros: standard pattern (Sparkle / WinSparkle / Squirrel.Windows); zero new launcher-side code beyond "swap the asset selector." Cons: full-installer download (larger than the binary-only zip we ship for self_update); UAC prompt per update unless setup.exe is code-signed with a trusted publisher.
  - **(B) HKCU + per-user install.** Move install dir from `$PROGRAMFILES64` to `%LOCALAPPDATA%\BriskaBlast\Launcher`, write the ARP keys under `HKCU` instead of `HKLM`. The launcher itself can then update `HKCU\…\Uninstall\BriskaBlastLauncher\DisplayVersion` after every self-update without UAC. Pros: invisible updates; matches the way Chrome/VSCode/Steam do per-user installs. Cons: NSIS rewrite, drops "Programs and Features for All Users" semantics, makes the install per-user-only (a multi-user machine needs separate installs per user account), doesn't help Linux at all.
  - **(C) Augment self_update to multi-file.** Ship a small "metadata-update" tarball alongside the binary zip: new `briskablast-launcher.exe` + new `Uninstall.exe` + a `.reg` file (or PowerShell script) that the launcher applies atomically post-swap. Pros: minimal change to the binary-swap mechanic; keeps per-machine HKLM install. Cons: still needs UAC for HKLM registry edits (so we elevate per update); most complex of the three to get atomic and rollback-safe; doesn't address Uninstall.exe being a running file's sibling.
  - **Linux equivalent.** The `.deb` install path has the analogous gap (`dpkg -l launcher` lags). The Debian-friendly answer is **not** to ship our own dpkg flow — it's to stand up a real apt repo (out of scope for foundation, real work for stable channel), and tell .deb-installed users their update channel is `apt update && apt upgrade` rather than the in-app updater. The `.AppImage` path has no analogous OS metadata to drift from, so the rename-trick stays correct there forever.
- **Related**:
  - Code-signing brainstorm (Azure Trusted Signing as cheapest viable cert) — same release window, bundle these together.
  - Identity file I/O (still deferred per [`launcher-foundation.md`](../launcher/launcher-foundation.md) §8 Open Items) — if option (B) lands, the identity-dir path question (`%APPDATA%` vs `%LOCALAPPDATA%`) gets re-litigated.
  - The `self_update` `.__old__*` orphan left in the install dir post-swap is a related but separate papercut: it's cleaned on next reboot (or next launch) regardless of which option above lands. Worth verifying whichever path is chosen also cleans the orphan as a side effect.

### server requirements for launcher to be able to contact servers

- should have a new field in the server to potential block calls to the server if the launcher is to out of date to safely handle the launcher and for itself

- Launcher Scans for existing games installs bottom check + option on first boot of launcher when no game file path directory found

### Game must always be launched by the launcher (reject standalone, bounce to launcher)

- **What**: A release-build game opened **standalone** (double-clicked directly, with no launcher handoff) should **refuse to run the game** and instead **open the launcher**, then quit. Each game channel is fundamentally dependent on the launcher — it supplies the identity (`player_id` + `secret_token`), the version handoff for the server's gate, the channel assertion, and now the `data_dir` for the single-instance file — and online play is impossible standalone anyway. So the game must always be launched *by* the launcher. On detecting a missing handoff (and **only** in a non-editor release build), the game would locate + spawn the installed launcher (or, if it can't be found, surface a clear "please launch from the BriskaBlast launcher" message) and exit before the main scene runs. This builds directly on the launcher→game handoff dependency established by the socket-rendezvous work (launcher v0.16.0 / game v0.15.0).
- **Why deferred / care needed**: The current build deliberately keeps two standalone paths that **must be preserved**: the DEBUG/editor self-register in `SessionContext.SelfRegisterAsync` (so two editor instances can host/join without the launcher) and the standalone data-dir fallback in `SingleInstance.FallbackDataDir`. The refuse-and-bounce behavior must therefore be **release-only AND gated on `!OS.HasFeature("editor")`**, or it would break the dev workflow. It also needs a reliable way for the game to *find* the launcher executable (the reverse of the handoff — e.g. a known install path, an OS registry/desktop entry, or a path the launcher records), plus a decision on the fallback UX when the launcher can't be located (clear message vs. silent quit). It's a UX/lifecycle change layered on the handoff dependency, not part of the single-instance mechanism itself, so it was kept out of that branch.
- **Trigger to start**: Hardening before a public/**stable** release where users may double-click the game exe directly, OR reports of players launching the game outside the launcher and hitting auth/version failures.
- **Related**: handoff producer/consumer in `launcher/src/game_launch/mod.rs` (`Handoff`) ↔ `client/src/core/LaunchArgs.cs` (`Handoff`, `FromLauncher`); editor self-register in `client/src/core/SessionContext.cs` (`SelfRegisterAsync`, DEBUG-only) and standalone fallback in `client/src/core/SingleInstance.cs` (`FallbackDataDir`) — both must stay for editor dev; channel is compile-time baked (`client/src/core/BuildConfig.cs`).

### Per-file hash manifest + deep Verify File Integrity — ✅ SHIPPED (game + launcher 0.17.0)

Shipped via a build-time `files.json` manifest (size + sha256 per file) and a two-pass deep Verify (presence+size, then sha256 on `spawn_blocking`), plus a **Repair** button (full reinstall of the installed version) and a Windows **Reset Runtime Cache** button. `VerifyOutcome` gained `FilesMissing` / `FilesCorrupted`. See [`docs/architecture/runtime-cache-and-integrity.md`](../architecture/runtime-cache-and-integrity.md) and the 0.17.0 changelogs.

- **Still deferred — "Re-download corrupted files only"**: Repair currently does a *full* reinstall, not surgical per-file re-download. GitHub Releases serves whole archive assets, so per-file fetch would need each file as its own asset (rate-limit cost) or range requests into the archive (impossible for `.tar.gz`). Wait for A/B install slots or real demand.

### Reset Runtime Cache on Linux / macOS

- **What**: Extend the Windows-only Reset Runtime Cache button to Linux/macOS once the on-disk runtime-cache location is confirmed there. On Windows the `.NET` runtime extracts to `%LOCALAPPDATA%\data_<name>_windows_x86_64`; on Linux/macOS it most likely lives inside the install dir / `.app` (where Verify/Repair already cover it), in which case Reset stays correctly N/A.
- **Why deferred**: No Linux/macOS tester to confirm the location — the launcher must not guess a path and risk deleting the wrong dir. `paths::runtime_cache_dir` returns `None` off Windows and the button isn't rendered there.
- **Trigger to start**: A tester runs `find ~ -type d -name 'data_BriskaBlast*'` on an installed+launched build; wire the confirmed path + `<platform>` suffix into `runtime_cache_dir`, or confirm N/A is final.

### One-time cleanup of the legacy un-suffixed runtime cache

- **What**: Auto-remove the orphaned `data_BriskaBlast_windows_x86_64` left on machines upgrading from a pre-0.17.0 **dev/ea** build (the old shared cache name).
- **Why deferred**: `data_BriskaBlast` is also **stable's** live cache name, so a blind delete during a dev/ea reset would nuke stable's cache. Must be gated on "no stable install on record" (stable recreates it on next launch if needed). Harmless ~80 MB orphan today; manual deletion for now.
- **Trigger to start**: Enough upgraded users accumulate orphans to matter, or a stable channel ships (raising the collision stakes).

### Settings "Add Firewall Rule" button (second entry point)

- **What**: An always-available, non-Play-coupled way to create the inbound rule, next to the existing "Check Firewall" row in Settings → Firewall. Enabled when the cached status is `NotDetected`.
- **Why deferred**: The first-Play prompt (shipped on `feat/firewall-elevation`) is the primary, lowest-surprise entry point. A Settings button is pure additive convenience — it duplicates the trigger without new mechanism.
- **Trigger to start**: User feedback that the first-Play prompt is being missed or reflexively dismissed, OR stable-channel polish.
- **Implementation notes**: Reuse `firewall::add_inbound_rule_elevated` (already built). Add an `AddFirewallRule(Channel)` message that runs it via `spawn_blocking` (same as the prompt's Allow path), and on `Ok` flip the cached `state.firewall_status` entry to `RulePresent` so the status cell updates without a re-check. The elevated call and arg-quoting are already done — this is just a second caller + a button in `settings.rs::firewall_section`.

### Hand-rolled firewall elevation FFI (drop the `runas` dependency)

- **What**: Replace the `runas` crate with a direct `windows-sys` implementation of the elevation: `ShellExecuteExW` (verb `runas`) + `WaitForSingleObject` + `GetExitCodeProcess`, plus our own argument-quoting helper.
- **Why deferred**: `runas` (v1.2.0, `windows-sys`-based) already does exactly this, quotes/escapes args correctly, and keeps our code free of `unsafe`. Hand-rolling is ~40 lines of `unsafe` FFI to review and maintain for no behavioral gain today.
- **Trigger to start**: `runas` goes unmaintained or breaks against a future `windows-sys`, we need behavior it doesn't expose, OR a dependency-minimization pass. `windows-sys` is already a transitive dep, so no new dependency is needed for the swap.
- **Related**: `launcher/src/firewall.rs::add_inbound_rule_elevated` (the single call site to replace).

### Persist firewall-prompt dismissal across launcher restarts

- **What**: Make the "Skip & Play" dismissal of the first-Play firewall prompt persistent per-channel, so a user who declined once isn't re-prompted on the next launcher launch while the rule is still missing.
- **Why deferred**: The shipped behavior uses an in-memory `firewall_prompt_dismissed` set that resets on restart — re-prompting next launch is defensible (the rule genuinely is still absent), and persisting it means an identity.json schema add. Polish, not correctness.
- **Trigger to start**: User annoyance reports about being re-prompted, OR the identity schema is being revised for another reason.
- **Related**: `launcher/src/app/state.rs` (`AppState::firewall_prompt_dismissed`), `launcher/src/identity.rs` (where a persisted per-channel flag would live).

### Saves-dir intact verify mode

- **What**: An alternative cheap variant of Verify File Integrity that confirms the executable exists AND `<install>/saves/` exists (creating it on demand if not). Catches the failure mode where a user clears the install dir but forgets `saves/`, or moves the install and leaves saves behind.
- **Why deferred**: Stage 7 takes the simpler exe-only path because the saves layout itself is still in flux — saves currently live colocated under the install dir for Stage 1 testing convenience, but the roadmap also tracks moving them to a platform-standard data dir for stable. Verifying the colocated layout would be wasted work if the dir moves.
- **Trigger to start**: Saves layout stabilises (after the platform-standard data dir migration) OR the keep-saves-on-uninstall flow accumulates real users whose backups end up orphaned.
- **Related**: `Saves dir relocation` (above) — both items land together once saves move out of the install dir.

### Consistent save-failure handling in the username-rename flow

- **What**: Make `confirm_username_change` (the launcher rename handler) mirror the safer ordering already used by `confirm_welcome_username` (first-register): clone the identity → set the new username → `identity::save` → return/log on save error → and only on a successful save commit `state.identity`, close the menu, and fan out `update_username` to the channel servers. Today the rename handler mutates in-memory state and notifies the servers even when the local save fails, so on-disk, in-memory, and server username can briefly diverge.
- **Why deferred**: Pre-existing latent inconsistency, not a live bug — a failed save during rename leaves only the *username* stale; `player_id`/`secret_token` stay intact and the server-canonical username reconciliation on the next `/register` heals the drift. The stricter clone-then-commit ordering is load-bearing only for first-register (reaching `/register` without an on-disk record would orphan the server identity), which is why the two handlers legitimately differ. Folding the change into the `app.rs` → `app/` refactor (PR #61) would have broken that PR's no-behavior-change contract, so it was split out here. Flagged by review on PR #61.
- **Trigger to start**: The identity-file schema or registration flow is reworked (raising the stakes of a half-saved rename), OR a user reports real username drift after a save failure.
- **Related**: `launcher/src/app/handlers/identity.rs` — `confirm_username_change` (handler to change) vs. `confirm_welcome_username` (the pattern to copy).

### Richer error typing for WS signaling reads (deferred from PR #62)

Two findings from the review of the `signaling/ws.rs` → `ws/` module split (PR #62). Both target code that was moved **verbatim** in that refactor, so neither is a regression introduced there — they are pre-existing, intentional simplifications. They were deliberately left out of PR #62 to preserve its no-behavior-change contract (the logic-token invariant check + changelog claim), and are recorded here to revisit together when a WS error-handling/observability pass is warranted.

- **`session_status_is_active` → `Result<bool, _>`** (`server/src/signaling/ws/disconnect.rs`). Today the fn returns `bool` and collapses Redis pool/GET/JSON-decode failures to `false` ("not active") — documented in its doc comment as fail-as-inactive. Proposal: return `Result<bool, _>`, propagate errors from `state.redis.get()`, `conn.get(...)`, and `serde_json::from_str::<Session>(&raw)` instead of mapping them to `false`, and have the sole caller (the joiner-drop branch in `ws/mod.rs::handle_socket`) treat `Err(_)` as a *transient* failure — i.e. still arm the reconnect grace / hold the slot rather than routing the joiner through the "announce leave" branch. Preserve the active check against `SessionStatus::Starting`/`Active`. *Impact is narrow:* a Redis blip exactly at a mid-game joiner's socket drop today routes them through "announce leave, keep slot" instead of "hold slot + reconnecting overlay" — both keep the slot, so the only divergence is the overlay/announcement, and only during a (rare) transient Redis fault.
- **`peer_roster` → descriptive error enum** (`server/src/signaling/ws/identify.rs`). Today returns `Result<Vec<String>, ()>`, collapsing pool errors, GET failures, missing key, and JSON-decode errors into `Err(())`. Proposal: a `PeerRosterError { RedisPool, RedisGet, NotFound, JsonDecode }` enum mapping each failure point (`state.redis.get()`, `conn.get(format!("session:{}", code))`, `raw.ok_or(...)`, `serde_json::from_str`) to its own variant, and update `handle_socket` to translate a genuine `NotFound` → `CLOSE_NOT_FOUND` vs. a backend fault → `CLOSE_INTERNAL`. Today every error collapses to `CLOSE_NOT_FOUND "session_gone"` — the caller comment already notes it's a "vanishingly rare race".

- **Why deferred**: Both are pre-existing, deliberate simplifications, not live bugs, and each is a client-visible behavior change (joiner-drop routing; WS close-code surface) that PR #62 explicitly excluded as a verbatim refactor.
- **Trigger to start**: A WS reconnect-hardening / error-observability pass, OR user reports of mid-game players announced gone that correlate with Redis hiccups, OR clients receiving `session_gone` (4404) when the real cause was a transient backend fault.
- **Related**: `server/src/signaling/ws/disconnect.rs::session_status_is_active`, `server/src/signaling/ws/identify.rs::peer_roster`, and their shared caller `server/src/signaling/ws/mod.rs::handle_socket`.

### Server-side validation of score reports (anti-forgery for match-end)

- **What**: Verify that a `ReportScore` reflects a legitimate goal before crediting it — server-side trajectory/state validation, or server-attested scoring — so a connected member can't forge score events to pad the tally or **trigger match-end** prematurely. Today `frame.rs::handle_client_frame` (the `ReportScore` arm) trusts any session member's report at face value; `SignalHub::record_score` only guards that the credited id is a *current room member*, not that the goal actually happened.
- **Why deferred**: The "trust any member's score report" model is **pre-existing and documented** — the `// Trusted for now` comment in `frame.rs`, the `ReportScore` doc in `signaling/protocol.rs`, and the Scoring section of `extended-mode.md` all already name trajectory validation as the later hook. The win-condition work (PR #78) did **not** introduce the forge-ability; it only **raised the impact** (a forged report can now end the match, not just inflate the scoreboard). A naive `reporter == scorer` guard does **not** fit this mode — by design the *scored-on* player reports a *different* player as the scorer — so there is no minimal authz patch; the real fix is the substantial trajectory-validation feature.
- **Source**: Flagged by **CodeRabbit** review on PR #78 (Set Score win condition).
- **Trigger to start**: An anti-cheat / trajectory-validation pass, OR reports of matches ending prematurely that correlate with a misbehaving / modified client.
- **Related**: `server/src/signaling/ws/frame.rs` (`ReportScore` arm), `server/src/signaling/mod.rs::record_score`, `server/src/signaling/protocol.rs` (`ReportScore` doc); overlaps with the **Score / replay / audit persistence** entry (an audit trail would feed validation).

### Game reserve fuction

Ball-loss watchdog — if the single ball died with the crashed process, the rejoined match has no ball until a watchdog re-serves it. Designed in the plan: ball holder broadcasts a BallAlive heartbeat; lowest-id connected player serves after a gap. Fast-follow.
Grace windows remain consts (runtime config later).