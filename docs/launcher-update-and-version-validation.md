# Launcher Self-Update & Server-Side Version Validation
### Architecture and Implementation Reference

---

## Overview

This document captures the architecture decisions and key insights for two interrelated systems:

1. **Launcher Self-Update** — how the launcher updates itself when new versions are released
2. **Server-Side Version Validation** — how the matchmaking server enforces version compatibility

These systems work together to ensure players are always running compatible code without surprise updates, cryptic protocol errors, or trust-the-client security holes.

---

## The Two-Layer Install Model

Compilation does **not** produce an installer. `cargo build --release` produces a binary that exists but is not registered with the OS — no entries in Programs and Features, no Start Menu shortcuts, no uninstall logic, no antivirus trust signals. The install story for this project requires a separate installer tooling layer.

### The Two Layers

| Layer | Purpose | Status |
|---|---|---|
| Layer 1 | OS-level installer that places the launcher on the user's system | To be added |
| Layer 2 | Launcher manages and installs game files | Already in architecture |

### Tooling Recommendations by Platform

| Platform | Recommended Tool | Notes |
|---|---|---|
| Windows | Inno Setup or NSIS | Free, generates setup.exe with proper Add/Remove Programs registration |
| Linux (Debian/Ubuntu) | `cargo-deb` | Generates .deb directly from Cargo project |
| Linux (portable) | AppImage | Single-file portable launcher |
| macOS | `cargo-bundle` + .dmg | Wrap .app in .dmg installer |
| Distribution platforms | Steam, Epic | Handle install/uninstall automatically — skip Layer 1 entirely |

### Uninstall Considerations

Specific to this architecture's identity system:

- **Game files + launcher binary** → delete on uninstall
- **Settings/config** → usually delete, sometimes ask
- **Player identity file** (`player_id` + `secret_token`) → **prompt user**

The identity file is intentionally separate from game saves per the original architecture. Losing it means losing the account permanently (no cloud saves planned). The uninstaller should present a "Keep player data for future reinstall?" checkbox.

---

## The Self-Update Core Puzzle

A running program cannot easily replace its own executable. On Windows the OS locks running `.exe` files. Every self-update technique is a different solution to one underlying question: *how does the running program get out of the way of its own file?*

### The Three Patterns

| Pattern | How It Works | Complexity |
|---|---|---|
| Updater Helper | Launcher exits → separate `updater.exe` performs the swap → relaunches the launcher | Medium |
| Bootstrap Stub | A tiny "stub" launches the real launcher; the real one is free to be replaced because it's not what the user "runs" | Medium-High |
| Rename Trick | Rename the running `.exe` (allowed even while locked on Windows), drop new `.exe` in original path, relaunch | Simple |

**Project decision:** Start with the **rename trick** via the `self_update` Rust crate. Migrate to the bootstrap stub only if the simpler approach causes friction.

### The `self_update` Crate

The Rust crate `self_update` handles:
- GitHub Releases API integration
- Binary download with progress callbacks
- Checksum verification (SHA256)
- The rename-and-swap-and-relaunch flow
- Cross-platform handling

Roughly 50 lines of integration code for the full flow. GitHub Releases acts as the source of truth — GitHub Actions builds binaries for each target on tag, publishes them to Releases with checksums, and the launcher queries the Releases API on demand.

---

## User-Initiated Update Flow

This project uses a **user-initiated** update model rather than aggressive auto-updates. Players retain agency over when updates happen.

### The UX Flow

```
1. User clicks "Check for Updates" in Settings (or sees Update Available badge)
    → Launcher queries GitHub Releases API
    → Compares latest release version to its own version

2. If newer version exists:
    → Show modal with release notes (pulled from GitHub Release body)
    → "Download and install?" Yes / No

3. User confirms:
    → Download with progress bar
    → Verify checksum against published SHA256
    → Show "Restart now?" prompt

4. User confirms restart:
    → CHECK FIRST: is the game running? If yes → refuse, warn user
    → Launcher renames itself, drops new binary into original path, relaunches
    → New launcher cleans up old renamed binary on startup
```

### Critical Safety Check: Game-Running Refusal

The launcher spawns the game process. Before initiating *any* update, check whether the game is active or in a session. Losing a multiplayer game session because someone tapped "Check for Updates" out of curiosity is a brutal UX failure and the kind of bug only discovered after shipping.

### Closing the Manual-Only Gap

Pure manual updates have a discoverability problem: users who never check Settings will run outdated launchers indefinitely. The project addresses this with two complementary safeguards:

**1. Silent check on launch (notification only)**
- Launcher quietly hits the Releases API on startup
- If newer version exists, show a small "Update Available" badge in the menu
- No interruption, no forced action — just discoverability

**2. Server-enforced minimum version**
- Server requires a minimum launcher version for online play
- Triggered at the moment being-current actually matters (joining a game)
- Surfaces the update prompt with clear context instead of cryptic protocol errors

Together these catch both the "wandering offline" and "trying to play online" cases without breaking the user-initiated principle.

---

## Server-Side Version Validation

### How the Version Travels

The launcher's version is **baked into the binary at compile time** via Cargo:

```toml
[package]
name = "launcher"
version = "1.0.0"
```

Accessible at runtime via `env!("CARGO_PKG_VERSION")`. Every released binary has its version permanently stamped — it cannot be lied about by the launcher's own code at runtime, because the value is compiled in.

The launcher sends its version with each server request as an HTTP header:

```
POST /join
Headers:
    X-Launcher-Version: 1.0.0
Body:
    { "session_code": "ABC123", ... }
```

### Server Validation Logic

The server's first action on each request is the version check. Conceptual flow:

```
fn handle_join(version_header, body):
    min_version = redis.get("min_launcher_version")
    
    if version_header < min_version:
        return 426 Upgrade Required {
            "error": "update_required",
            "minimum_version": min_version,
            "current_version": version_header
        }
    
    # ... continue with normal join logic
```

**HTTP 426 Upgrade Required** is the standardized status code for this case — the HTTP spec literally created it for this purpose.

### Launcher Response Handling

```
Got 200 OK              → Proceed with session join
Got 426 Upgrade Required → Show update prompt, do not retry
Got 400 / 401           → Protocol or auth error (see security model below)
```

---

## The Protocol-Level Enforcement Principle

### The Critical Insight

**The version header is a convenience layer, not a security gate.**

A hacked launcher could trivially lie about its version by patching the header value. This does not matter, because the actual security is enforced by the protocol itself, not by the version label.

### Why It Works

When a launcher is updated from 1.0.0 to 1.5.0, what actually changes is *how the launcher talks to the server* — new fields in requests, different structure, new validation rules on the server side. The "1.5.0" label just names that protocol shift.

A hacker who hex-edits launcher 1.0.0 to claim it's 1.5.0 has not gained any of 1.5.0's actual capabilities. The code generating the request body is still 1.0.0's code, and that code doesn't know about the new fields. The request body itself betrays the actual version of the code that produced it.

### Worked Example

**1.0.0 protocol — what the request looks like:**
```json
{ "session_code": "ABC123" }
```

**1.5.0 protocol — added identity verification:**
```json
{
    "session_code": "ABC123",
    "player_id": "0000001",
    "secret_token": "k9mX2$nP8qL..."
}
```

**Scenario A: Honest 1.0.0 user hits new server**
- Sends old-format request with version header 1.0.0
- Server sees version below minimum → returns `426 Upgrade Required`
- Launcher shows "Update required to play online"
- *Friendly, clear user experience*

**Scenario B: Hacker patches launcher to lie about version**
```
Headers: X-Launcher-Version: 1.5.0    ← the lie
Body:    { "session_code": "ABC123" }  ← still 1.0.0 shape
```
Server runs normal validation:
- `player_id` missing → reject
- Returns `400 Bad Request: missing required fields`

The version lie did not help. The body betrays the actual code.

**Scenario C: Hacker also fakes the new fields**
```json
{
    "session_code": "ABC123",
    "player_id": "9999999",
    "secret_token": "i_made_this_up"
}
```
Server looks up `player_id: 9999999` in its database, hashes `i_made_this_up`, compares to stored hash → no match → `401 Unauthorized`.

To pass this check, the hacker would need a *real* player_id paired with its *real* secret_token. Those are bound server-side. The real token never leaves the legitimate owner's machine. Faking the pair is computationally infeasible.

### The Architectural Principle

**Trust no client input. Validate everything server-side.**

This is the same principle already applied in this project's score validation system: the server independently calculates expected ball trajectories rather than trusting score claims from clients. The version-protocol pattern applies the same principle to a different problem.

The server holds the source of truth for:
- What the protocol currently looks like
- What player identities actually exist
- What ball physics should produce
- What versions are currently acceptable

None of these can be faked from the client side.

---

## Dynamic Configuration

### The Problem with Hardcoded Minimums

If the minimum required version is hardcoded in the server source code, changing it requires:
- Code change
- Rebuild
- New Docker image
- Push to registry
- Container redeploy

This is too slow for time-sensitive situations such as a critical security fix that requires immediate forced update.

### The Solution: Runtime Configuration

Store `min_launcher_version` as a **runtime value** in Redis. The server reads it on every request (with optional in-memory caching for performance).

```
Redis key:   min_launcher_version
Redis value: "1.2.0"
```

### Update Flow

```
Critical bug discovered in launcher 1.1.0
    → Admin opens Portainer or admin web page
    → Changes min_launcher_version from "1.1.0" to "1.2.0"
    → Within seconds, all outdated launchers receive 426 responses
    → Users see "Update required" prompts at next online action
    → No code change, no redeploy, no downtime
```

### Admin Interface Options

| Option | Pros | Cons |
|---|---|---|
| Direct Redis edit via Portainer | No code needed | Requires Redis CLI/GUI knowledge |
| Small password-protected admin endpoint | Clean UX, auditable | Requires building a small admin UI |
| Environment variable + container restart | Visible in Portainer | Brief downtime on change |

**Recommendation:** Small password-protected admin endpoint that updates Redis. Provides clean UX, easy audit trail, and zero downtime.

### The Generalizable Pattern

This dynamic configuration approach applies to many runtime decisions beyond version checks:

- Feature flags (enable / disable game modes)
- Maintenance mode (block new sessions during deploys)
- Rate limits (adjust without restart)
- Banned player IDs (instant ban without code change)
- Region-specific minimums (different minimums per geography)

**The general principle:** separate **code** (changes slowly, requires deploys) from **configuration** (changes fast, requires no deploy). This is a foundational pattern in production systems.

---

## Connection to Existing Architecture

This update and version system integrates cleanly with the existing project architecture. No new infrastructure is required.

| Existing Component | Role in Update System |
|---|---|
| Rust launcher | Hosts the self-update logic via `self_update` crate |
| Rust + Axum server | Validates versions, returns 426 when incompatible |
| Redis | Stores `min_launcher_version` as dynamic config |
| Portainer | Admin interface for changing minimum version |
| GitHub Actions | Builds and publishes launcher binaries to Releases |
| Identity system (player_id + secret_token) | Used in protocol validation; cannot be faked |
| Score validation system | Same architectural principle: server-side validation |

The update system is entirely additive — every piece slots into infrastructure that's already part of the project plan.

---

## Implementation Specifics

### Launcher Side (Rust)

**Cargo.toml dependency:**
```toml
[dependencies]
self_update = { version = "0.39", features = ["archive-tar", "compression-flate2"] }
reqwest = { version = "0.11", features = ["json"] }
semver = "1.0"
```

**Version constant access:**
```rust
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");
```

**Sending version on requests:**
```rust
client.post("https://server.example.com/join")
    .header("X-Launcher-Version", LAUNCHER_VERSION)
    .json(&request_body)
    .send()
    .await?
```

**Update check via self_update:**
```rust
let status = self_update::backends::github::Update::configure()
    .repo_owner("your_org")
    .repo_name("game_launcher")
    .bin_name("launcher")
    .current_version(LAUNCHER_VERSION)
    .build()?
    .update()?;
```

**Handling 426 response:**
```rust
match response.status() {
    StatusCode::OK => proceed_with_session(response).await,
    StatusCode::UPGRADE_REQUIRED => show_update_prompt(response).await,
    StatusCode::UNAUTHORIZED => show_auth_error(),
    _ => show_generic_error(),
}
```

### Server Side (Rust + Axum)

**Version check middleware concept:**
```rust
async fn version_check_middleware(
    headers: HeaderMap,
    State(redis): State<RedisClient>,
    request: Request,
    next: Next,
) -> Response {
    let version = headers.get("X-Launcher-Version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0.0.0");
    
    let min_version: String = redis
        .get("min_launcher_version")
        .await
        .unwrap_or_else(|_| "0.0.0".into());
    
    if !meets_minimum(version, &min_version) {
        return (
            StatusCode::UPGRADE_REQUIRED,
            Json(json!({
                "error": "update_required",
                "minimum_version": min_version,
                "current_version": version
            }))
        ).into_response();
    }
    
    next.run(request).await
}
```

**Version comparison:** Use the `semver` crate for proper semantic version comparison. Never compare versions as raw strings — `"1.10.0" < "1.9.0"` is `true` in string comparison, which is wrong.

---

## Build Order Position

This work slots into the existing project build order as follows:

```
1. Server first              (existing decision)
   └── Version check middleware added before any real users
2. Game second               (existing decision)
3. Launcher third            (existing decision)
   ├── Game install logic
   ├── Settings management
   ├── Identity file handling
   └── Self-update logic    ← added here
4. Layer 1 installer last    (Inno Setup / cargo-deb / etc.)
```

**Self-update logic is added to the launcher before any release goes out to real users.** You do not want anyone running v1 with no path to v2.

---

## Future Considerations (Deferred)

These items are intentionally deferred to keep scope manageable in the foundation phase:

- **Cryptographic binary signing** (OS-level code signing for launchers) — not needed in foundation phase because protocol-level enforcement already provides the actual security
- **Delta updates** (downloading only changed bytes via tools like bsdiff or zstd dictionaries) — optimization, not required initially
- **Rollback mechanism** (revert if new version crashes on first run) — useful but adds complexity
- **Multiple update channels** (stable / beta / nightly) — possible future enhancement
- **Bootstrap stub launcher** — migrate to this only if rename trick causes friction
- **Steam / Epic integration** — would replace Layer 1 installer entirely, also replaces identity system

---

## Key Concepts Learned

| Concept | Definition |
|---|---|
| Two-Layer Install Model | OS-level installer for launcher; launcher itself installs game |
| Self-Modification Problem | A running program cannot directly replace its own executable |
| Rename Trick | Rename the running .exe, drop new binary in original path, relaunch |
| Bootstrap Stub | Tiny stub launches the real launcher, allowing the real one to be freely replaced |
| Updater Helper | Separate process performs the binary swap after launcher exits |
| Compile-Time Version | Version baked into the binary via Cargo.toml, accessible at runtime via `env!("CARGO_PKG_VERSION")` |
| HTTP 426 Upgrade Required | Standard HTTP status code for "client version too old" |
| Dynamic Configuration | Runtime values changeable without code deploy |
| Protocol-Level Enforcement | Security enforced via request structure, not version label |
| Courtesy Label | The version header's actual role — enabling friendly error messages, not security |
| Trust No Client Input | Core principle: server validates everything independently |
| Source of Truth | Server-side data the client cannot fake (identity tokens, valid protocols, accepted versions) |
| Semantic Versioning (Semver) | Major.Minor.Patch versioning (e.g., 1.2.3); always compare with the `semver` crate, never as strings |
