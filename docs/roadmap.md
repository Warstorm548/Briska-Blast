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
