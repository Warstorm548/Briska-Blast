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
