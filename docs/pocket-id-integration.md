# Pocket ID Integration — Admin Panel

> Status: **Planned / Not Started**

Pocket ID is a self-hosted OIDC provider that uses passkeys only (no passwords).
This document captures everything needed to wire it into the BriskaBlast admin panel when we return to this work.

---

## What Pocket ID Provides

**OIDC Endpoints** (relative to the Pocket ID instance base URL):

| Endpoint | Purpose |
|---|---|
| `/oidc/authorize` | Start auth — redirect user to Pocket ID login |
| `/oidc/token` | Exchange authorization code for tokens |
| `/oidc/userinfo` | Get user claims from an access token |
| `/oidc/end-session` | Logout on the Pocket ID side |
| `/.well-known/openid-configuration` | Standard OIDC discovery |

**Flow to use**: Authorization Code + PKCE (correct for a server-side web app).

**Scopes and claims**:

| Scope | Claims returned |
|---|---|
| `openid` | `sub` (user ID) |
| `email` | `email`, `email_verified` |
| `profile` | `given_name`, `family_name`, `name`, `preferred_username`, `picture` |
| `groups` | `groups` — array of group names the user belongs to |

There is **no explicit admin role claim**. Authorization is group-based: check that the user's `groups` array contains the configured admin group name.

---

## One-Time Setup in Pocket ID

Before any code runs, an admin must do this in the Pocket ID web UI:

1. Go to **Settings → OIDC Clients** and create a new client for BriskaBlast.
2. Set the redirect URI to `https://<your-admin-domain>/admin/oidc/callback`.
3. Request scopes: `openid email profile groups`.
4. Copy the generated **Client ID** and **Client Secret**.
5. Create a user group (e.g. `briska_admins`) and add the admin user(s) to it.

---

## New Environment Variables Needed

Same pattern as `GAME_PORT` / `ADMIN_PORT` — deployment-time config via `.env` / `docker-compose.yml`, not runtime-managed.

| Variable | Description | Example |
|---|---|---|
| `POCKET_ID_URL` | Base URL of the Pocket ID instance | `https://id.example.com` |
| `OIDC_CLIENT_ID` | Client ID from Pocket ID registration | (UUID) |
| `OIDC_CLIENT_SECRET` | Client secret from Pocket ID registration | (random string) |
| `OIDC_ADMIN_GROUP` | Pocket ID group name that grants admin access | `briska_admins` |

The feature should be **opt-in**: if `POCKET_ID_URL` is not set, the admin panel falls back to the existing password auth unchanged.

---

## New Routes Required (Admin Axum Listener)

### `GET /admin/oidc/login`
1. Generate a cryptographically random `state` value and a PKCE `code_verifier`.
2. Compute `code_challenge = BASE64URL(SHA256(code_verifier))`.
3. Store `state → code_verifier` in Redis with a short TTL (~5 minutes), keyed as `admin:oidc_state:<state>`.
4. Redirect the browser to:
   ```
   {POCKET_ID_URL}/oidc/authorize
     ?response_type=code
     &client_id={OIDC_CLIENT_ID}
     &redirect_uri=https://<admin-domain>/admin/oidc/callback
     &scope=openid email profile groups
     &state={state}
     &code_challenge={code_challenge}
     &code_challenge_method=S256
   ```

### `GET /admin/oidc/callback`
1. Read `code` and `state` from query params.
2. Look up `admin:oidc_state:<state>` in Redis — if missing or expired, return an error (CSRF protection).
3. Delete the state key from Redis (one-time use).
4. POST to `{POCKET_ID_URL}/oidc/token`:
   ```
   grant_type=authorization_code
   code=<code>
   redirect_uri=https://<admin-domain>/admin/oidc/callback
   client_id=<OIDC_CLIENT_ID>
   client_secret=<OIDC_CLIENT_SECRET>
   code_verifier=<stored code_verifier>
   ```
5. Parse the returned `id_token` (JWT — decode without signature verification for now, or verify against Pocket ID's JWKS).
6. Extract the `groups` claim. If it does not contain `OIDC_ADMIN_GROUP`, return 403.
7. Generate a random 32-byte session token, store it in Redis as `admin:session:<token>` with 24h TTL (same pattern as password auth).
8. Set `briska_admin_session` cookie and redirect to `/admin/dashboard`.

---

## New Rust Dependencies

| Crate | Purpose |
|---|---|
| `reqwest` (with `json`, `rustls-tls` features) | HTTP calls to Pocket ID token endpoint |
| `base64` | PKCE `code_challenge` encoding (base64url) |
| `jsonwebtoken` | Decode and optionally verify the ID token JWT |

**Alternative**: The `openidconnect` crate handles discovery, token validation, and JWKS verification automatically — heavier but safer. Either works; hand-rolling with `reqwest` + `jsonwebtoken` is simpler for our narrow use case.

---

## Changes to Existing Code

| File | Change needed |
|---|---|
| `server/src/admin/mod.rs` | Add OIDC config fields to `AppState` (optional, populated from env vars) |
| `server/src/admin/auth.rs` | Add `oidc_login` and `oidc_callback` handlers |
| `server/src/admin/templates.rs` | Add "Login with Pocket ID" button to `login_page()` |
| `server/src/main.rs` / router | Register new OIDC routes |
| `server/Cargo.toml` | Add `reqwest`, `base64`, `jsonwebtoken` |
| `.env.example` / `docker-compose.yml` | Document the four new env vars |

---

## Open Decisions (resolve before coding)

1. **Coexist or replace?** Should Pocket ID be an *alternative* login alongside the password form, or fully replace it? Replacing is cleaner but requires Pocket ID to be reachable for any admin access.
2. **Group requirement?** Require a specific Pocket ID group, or allow any authenticated Pocket ID user in?
3. **JWT verification depth?** Decode-only (trust the token came from the expected redirect) vs. full signature verification against Pocket ID's JWKS. Full verification is more correct but adds complexity.
