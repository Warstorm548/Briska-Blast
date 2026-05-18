# Changing the Update System Safely

## When to read this

Open this document before you change any of the following in the server:

- Files under `server/src/update/`
- `server/src/state.rs` fields used by the update path (`update_tx`, `update_apply_lock`)
- `server/src/config.rs` fields consumed by the update path (`watchtower_url`, `watchtower_token`, anything new you add)
- `docker-compose.yml` — *any* change to the `server` or `watchtower` services
- `.env.example` — adding or removing variables the binary reads

If you're touching any of the above, walk the **[Pre-merge checklist](#pre-merge-checklist)** before merging. The rest of this doc explains *why* each item exists.

---

## The fundamental rule

> **vN's code is what actually performs the update to vN+1.**
> vN+1's code only runs *after* the container has restarted.

Two corollaries that should be tattooed onto the inside of your eyelids:

1. **Any new behaviour you add to the update path in vN+1 doesn't help you *get to* vN+1.** It only matters for vN+1 → vN+2 and beyond.
2. **Any bug you introduce into the update path in vN+1 can strand the server permanently** — if vN+1 can't trigger Watchtower, vN+2 will never apply, and the only remedy is SSH-and-fix.

This is why the update path is fundamentally different from every other piece of code in the server. A bug in the matchmaking endpoint can be fixed by shipping a patch. A bug in the update path may prevent that patch from ever being applied.

---

## Pre-merge checklist

Walk this list before merging any PR that touches the files listed in [When to read this](#when-to-read-this).

- [ ] **Will an existing running server (the previous stable version) be able to discover this release?**
      vN's `github::check_for_update` must still parse the GitHub Releases response and detect that this tag is newer. If you changed parsing, can vN still handle the JSON?
- [ ] **Will an existing running server be able to apply this release?**
      vN's `task.rs::ApplyNow` must still successfully trigger Watchtower against this release's image. If you changed the Watchtower contract (URL, auth header, HTTP API), vN doesn't know about the change.
- [ ] **Does this release add any required env var the binary refuses to start without?**
      If yes, the auto-update will succeed-then-crash on first boot. Treat as an **operator-attended release** — release notes must say "after auto-update, run `docker compose up -d`". Better: ship one release where the new var is optional with a sensible default, then make it required in a *later* release once compose has been updated everywhere.
- [ ] **Does this release rename or remove any env var the binary reads?**
      Same risk as above. Support both old and new names for one transition release before removing the old.
- [ ] **Does this release change the *meaning* of an existing Redis key under `update:*`?**
      If yes, rework the change to use a new key instead. Repurposing keys breaks both forward and backward (rollback) paths.
- [ ] **Does this release change `docker-compose.yml`?**
      Watchtower swaps container *images*, not stack topology. Compose changes only take effect on `docker compose up -d`. Treat as operator-attended.
- [ ] **Has the "Apply Now" path been manually verified on staging with this release as the target?**
      Tag a prerelease (`-ea.N`), let it auto-update onto staging, watch the apply complete end-to-end. **No release-time skipping this step for update-path changes.**
- [ ] **Does `Cargo.toml` version match the git tag you're about to push?**
      `CARGO_PKG_VERSION` is baked into the binary at compile time and drives the "newer than running" comparison. A mismatch means the server thinks it's already up to date forever.

If any box above is unchecked, the merge isn't ready — either fix the issue, downgrade the change to a non-breaking variant, or escalate to operator-attended.

---

## Categories of change, by risk

### ✅ Safe — ship freely

| Change | Why it's safe |
|---|---|
| Pure logic changes in `update/*.rs` that improve behaviour | vN applies vN+1 using vN's code; the new behaviour matters only from vN+1 onward |
| Adding new Redis keys under `update:*` | vN doesn't read them; vN+1 reads them as missing → empty → safe |
| Better error handling / structured logging in the update path | No behavioural contract change |
| New admin buttons / endpoints | vN doesn't know they exist; vN+1 just adds them |
| New *optional* env vars with a default fallback in `config.rs` | If `.env` doesn't set them, the default applies; old compose works |

### ⚠️ Risky — needs care

| Change | The risk | Mitigation |
|---|---|---|
| Repurposing the meaning of an existing Redis key | vN wrote one format; vN+1 reads another and silently parses wrong | Don't. Always add a *new* key; deprecate the old over multiple releases |
| Renaming env vars the binary reads | vN container has the old name set; vN+1 looks for the new name and sees nothing | Read both names for one transition release with a `tracing::warn!` on the old, then drop the old later |
| Bumping `Cargo.toml` version without matching the git tag | `CARGO_PKG_VERSION` mismatches GHCR's tags; "Already up to date" forever | Tag from the version, or assert match in CI |
| *Any* bug in `task.rs::ApplyNow` or `wait_and_apply` | vN+1 finds vN+2 but can't trigger Watchtower; server is stuck forever | Manual staging verification before tagging stable |

### 🛑 Dangerous — requires an operator step

| Change | The risk | Required action |
|---|---|---|
| New *required* env var the binary refuses to start without | Watchtower restarts the container with old env (no new var) → vN+1 crashes on boot → `restart: always` loops forever | Two-phase: ship optional first, required later. OR treat this single release as operator-attended with a `docker compose up -d` step in release notes |
| New sidecar service in compose (e.g., the deferred Docker socket proxy) | The server binary may reference a service not in the running network | Operator-attended release with `docker compose up -d` |
| Changing a port or removing a service | Binary's environment expectations diverge from running compose | Operator-attended release |
| Anything that requires `RELEASE_CHANNEL` to change | `RELEASE_CHANNEL` is baked in at compile time; can't change without rebuild + image swap | Not really a self-update concern — requires a redeploy |

### 💀 Catastrophic — avoid entirely

| Change | What goes wrong |
|---|---|
| Changing the GitHub Releases JSON `serde::Deserialize` shape in a way that breaks vN's parsing | vN can't discover updates anymore — stuck on vN until SSH-fixed |
| Breaking `watchtower::trigger_update`'s URL format or auth header | vN can't trigger Watchtower — stuck |
| Introducing an infinite loop / panic in the update task | Update task dies; no more polling — stuck until container restart, then stuck again |
| Removing the Docker socket mount from the server service | Both `pull_channel_image` and `retag_for_rollback` lose their backend — auto-apply silently no-ops, rollback fails |

---

## Patterns that keep changes safe

### 1. Treat the update path like a public API

Anything in `update/`, any `AppState` field used by `update/`, any compose env var consumed by `update/` is part of an irrevocable contract with every already-deployed version. You cannot fix a broken contract via an update because the broken version is what would do the fixing.

### 2. Always add, rarely change, never remove during one release

Spread breaking changes over multiple releases:

- **Release N**: add new behaviour alongside old; no removal
- **Release N+1**: prefer the new path internally; deprecate the old (warn on use)
- **Release N+2**: remove the old path

This guarantees every adjacent-version pair (N→N+1, N+1→N+2, …) works under the auto-update flow.

### 3. Use a staging environment that mirrors prod's compose

The cheapest insurance: a `staging` compose stack on the same dedi (different port triplet — `25929/25930/25931`), pulling from the `:ea` channel. Tag a prerelease (`v0.4.2-ea.1`), let it auto-update onto staging, watch the apply complete via `docker compose logs -f server`. If anything breaks, you find it before prod sees it.

This costs you one extra git tag and roughly five minutes per release. Worth it.

### 4. Reserve version bumps to signal what kind of release this is

A practical convention:

| Bump | What it means | Operator action required? |
|---|---|---|
| **Patch** (`0.4.1` → `0.4.2`) | Pure code; only files under `server/src/`. No compose changes. No new env vars. No new Redis key meanings. | None. Fully automatic via self-update. |
| **Minor** (`0.4.x` → `0.5.0`) | May include compose changes; may include new optional env vars; may include new sidecar services. | Release notes specify what step is needed (typically `docker compose up -d` after auto-update completes). |
| **Major** (`0.x.x` → `1.0.0`) | Potentially incompatible Redis schema or breaking protocol changes. Migration step required. | Full operator-attended deploy following a documented migration runbook. |

Operators reading release notes for a minor bump know to plan the extra command. Operators reading patch-release notes know they can let it ride.

### 5. Keep a manual recovery path in the repo

`tools/manual-deploy.sh` (or its equivalent) should exist and be tested. If self-update ever strands a server, the operator SSHes in and runs:

```bash
cd ~/briska/prod
git pull
docker compose pull
docker compose up -d --force-recreate
```

This is the universal "get out of jail" sequence and should never be the first step of a normal deploy — but it must work when needed.

---

## A worked example: adding the Docker socket proxy

Finding 11 in the v0.4.1 changelog is deferred to a future branch `harden/docker-socket-proxy`. Walking that future change through the framework above:

- **Adds a new sidecar service** (Docker socket proxy) → 🛑 operator step required
- **Server's `docker-compose.yml` will change to mount the proxy socket instead of `/var/run/docker.sock`** → 🛑 compose change
- **No new env vars** → ✅
- **No Redis key changes** → ✅
- **No `update/*.rs` logic changes** → ✅ binary update itself is safe

**Recommended release plan:**

1. Build and test the proxy integration on staging.
2. Tag `v0.5.0-ea.1` (minor bump because of compose) on the `harden/docker-socket-proxy` branch.
3. Auto-update onto staging. Verify:
   - The server's binary boots (compose hasn't changed yet, so the proxy socket isn't mounted — but the binary is unchanged from v0.4.x's perspective)
   - At this point staging is "half-deployed": new binary, old compose
4. Run `docker compose up -d` on staging. Verify:
   - Proxy sidecar starts
   - Server container restarts with the proxy socket mount
   - Rollback test: trigger from admin panel, confirm retag succeeds via the proxy
5. Promote to stable: tag `v0.5.0`.
6. Release notes for `v0.5.0` lead with: **"This release changes the compose stack. After auto-update, you MUST run `docker compose up -d` on each deployment to enable the new Docker socket proxy. The server will continue to function before that step, but rollback will not work until the proxy is in place."**

---

## What to do when you're not sure

If a planned change doesn't obviously fit one of the categories above, **assume it's risky and ask before merging.** It's much cheaper to delay a feature by a day than to debug a stranded production server.

Concrete escalation: open a draft PR, link this document, and tag it with a "needs update-path review" label. A second pair of eyes specifically on the questions in the [Pre-merge checklist](#pre-merge-checklist) is the single highest-leverage code review you can do on this project.
