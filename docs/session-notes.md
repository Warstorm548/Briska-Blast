# Session Notes

Personal reference — things to pick up next session.

---

## To Investigate

### 1. Admin Panel — Pocket ID Support
- Determine what "pocket ID" means in context of the admin panel
- What data needs to be displayed / searchable
- Does the admin panel need to look up players by ID, ban them, view session history?

### 2. Per-Server Auto-Update System
- Evaluate how each server instance identifies itself (server type tag?)
- Each instance should pull the correct Docker image on push (not a shared image)
- **Admin panel additions needed:**
  - Auto-update toggle (on/off per server or global)
  - When toggle is OFF: manual "Check for Updates" button
    - Checks GitHub for a new image matching this server's type
    - Prompts admin to apply or dismiss
  - When toggle is ON: update happens automatically on new push
- Questions to answer first:
  - How are server types defined? (env var? Redis key?)
  - Where does the Docker image tag come from? (GitHub Container Registry?)
  - How does the server know it's outdated? (GitHub API poll? Webhook?)
