# Roadmap

Tracks deferred work and decisions made out of the current sprint.
For *how* an item should be built, follow the link to its spec doc.

---

## Post-Deployment Follow-Ups

Work intentionally deferred until after the initial production deployment of the server (v0.4.1).

### Pocket ID admin SSO

- **What**: Replace the bcrypt password login at `/admin/login` with Pocket ID OIDC (passkey-based SSO).
- **Why deferred**: First deployment risk is already high; adding an external identity provider widens the failure surface. Current bcrypt + Redis session auth is adequate for a single operator. Migration is cheap later because the auth surface is small and isolated — only `server/src/admin/auth.rs` and one route change; the dashboard, update system, and version controls all just check `require_session`.
- **Trigger to start**: A second admin user needs access, OR the operator has time to stand up the Pocket ID instance as production infra.
- **Spec**: [`pocket-id-integration.md`](pocket-id-integration.md)

### Per-gamemode host-configurable player range

- **What**: Extend the session-creation API so the host can send a `[min, max]` pair (instead of a single `player_count`) when the selected gamemode supports a host-configurable range. The server would still re-validate the pair against the gamemode's authoritative `[min_players, max_players]` bounds.
- **Why deferred**: The initial session-validation work uses a single value because no current gamemode benefits from a host-chosen range. Locking in a pair schema now would couple the request format to gamemodes that don't exist yet and force every gamemode to adopt the same shape even when it isn't meaningful. Easier to extend the schema once a real driving gamemode lands.
- **Trigger to start**: A new gamemode is added whose intended UX requires the host to pick a sub-range inside the gamemode's allowed bounds (e.g. a mode that legitimately supports anywhere from 3 to 6 players and the host wants to cap their lobby at 4).

### TURN relay for symmetric-NAT players

- **What**: Add a TURN server (Cloudflare TURN free tier, or self-hosted coturn) and issue short-lived TURN credentials to clients in `Identified` / `start_signaling` payloads. Update the test harness and (eventually) the Godot client to include the TURN URL in their `RTCPeerConnection` `iceServers` list. Symmetric-NAT players can then route through TURN instead of being kicked.
- **Why deferred**: Roughly 5–10% of consumer routers are symmetric NATs. Shipping without TURN means that minority cannot play, but adding TURN requires either a vendor account (Cloudflare API key + credential issuance endpoint) or self-hosted infrastructure (coturn container, firewall ports, abuse hardening). Neither is a one-evening change, and the symmetric-NAT player can still verify the server works with peers who are reachable. Better to ship the signaling backbone first and revisit once the audience is large enough to justify the operational cost.
- **Trigger to start**: User reports of "can't connect" that correlate with NAT type, OR the Godot client lands and we want to broaden compatibility before public beta.
- **Related**: see the symmetric-NAT entries in [`session-multiplayer-edge-cases.md`](session-multiplayer-edge-cases.md).

### WS-ticket auth (stop sending `secret_token` over the WebSocket)

- **What**: The launcher / client currently sends its `secret_token` as cleartext in the first WS `Identify` frame. Replace that with a short-lived signed ticket: `/host` and `/join` REST responses include a `ws_ticket` (HMAC-signed `{player_id, session_code, expiry}`), and the client opens the WebSocket as `/ws/session/{code}?ticket=...`. The server verifies the signature + expiry on upgrade; the raw token never crosses the WS.
- **Why deferred**: Production deployment will use `wss://` (TLS) which already protects the token in transit. Tickets are defense in depth, but not load-bearing yet — and shipping ticket infra requires signing-key management, expiry handling, and a launcher-side change. Easier to land after the launcher exists and we can update both ends together.
- **Trigger to start**: Security audit before any non-TLS deployment path opens up, OR the Godot client lands (good moment to define this part of the wire shape once for both crates).
- **Related**: see the WS auth replay entry in [`session-multiplayer-edge-cases.md`](session-multiplayer-edge-cases.md).
