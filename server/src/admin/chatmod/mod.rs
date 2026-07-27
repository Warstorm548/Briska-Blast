//! The admin **Chat-Mod** tab — chat moderation.
//!
//! Handlers here resolve a session and render; the live data they show comes from
//! [`super::chatmod_data`], which projects the Redis-backed storage in
//! [`crate::chat`] onto the template view models.
//!
//! **Storage is Redis, not SQLite.** An earlier iteration of this design named a
//! SQLite moderation database; that is shelved. Everything uses the same Redis
//! patterns as the rest of the server, and no chat key carries a TTL —
//! `session:{CODE}` is the only key in the deployment that expires.
//!
//! Wired so far: the live transcript, body-id assignment, blacklist matching and
//! censoring, the flagged red dot, the Blacklisted Words tab and its Quick Access
//! twin, moderator chat, **Warn** and **Warn + Delete Chat Body**, and all four
//! audit categories. **Not** wired: Suspend, Ban, Approve Word, the
//! Banned/Suspended lists, audit filters, and manual deletion of retained
//! records. The contracts below describe the whole design, including the parts
//! still to come.
//!
//! Warning delivery is live-only and never queued: a player who is disconnected,
//! or who is in a match (chat renders only in the lobby), does not receive it,
//! and the attempt is recorded as undelivered. Deleting a body withdraws it from
//! every connected player but removes nothing server-side — the transcript is
//! append-only because audit snapshots pin cut indices into it, so a deletion is
//! a mark in `chat:deleted:{sid}` and the moderator keeps seeing the message,
//! greyed.
//!
//! Censoring contract for the wiring phase: when the server flags a
//! blacklisted word, the game side renders it blacked/hashed out immediately.
//! "Approve Word" un-censors the selected occurrence(s) in that chat — for
//! words that are permissible in that sentence's context — but it never
//! removes the word from the blacklist itself; future occurrences are
//! censored again and re-reviewed case by case.
//!
//! Audit-log contract for the wiring phase: every player-actionable tool
//! (Warn + Delete, Warn Only, Suspend, Ban) writes an audit record containing
//! the reason, the target's username + player id, the **set of message bodies
//! the action covered**, and a snapshot of the chat history as it stood when
//! the action was taken. Any tool — not just Warn + Delete — may act on zero,
//! one, or several of the target's bodies at once (e.g. a Warn/Suspend/Ban that
//! cites multiple messages), so the record's body list is always a `Vec`, never
//! a single id; `AuditEntry.body_ids` already reflects this. Granularity: one
//! record per **(action instance, target player)** — a single press hitting
//! several players splits into one record per player (each carrying that
//! player's covered bodies), and repeated presses on the same player are never
//! merged (a player may recur across a session). Records live in Redis
//! (`chat:audit:*`, see [`crate::chat::audit`]) and surface in the Chat Audit
//! Logs area of the Chat Nav. A record does not embed the chat history: it pins
//! the transcript instance plus a cut index, and rendering replays it.
//! Ban additionally requires an explicit confirmation dialog — a cancelled
//! confirmation sends nothing and writes no audit record.
//!
//! Scope contract: every player action binds to the player account, not the
//! session it was issued from, and governs CHAT privileges (not game access):
//! - Suspend = temporary chat mute for the entered duration — the player
//!   cannot chat in game lobbies, nor in the play field if chat moves there
//!   later, but keeps playing.
//! - Ban = permanent loss of chat privileges, lifted only by removing the
//!   player from the ban list (managed via the Moderation Lists area).
//!
//! Both apply across all sessions. Only message-body operations (delete,
//! per-occurrence word approval) are scoped to their session's chat.
//!
//! Split by concern: the page renderers live in [`pages`], the live-refresh
//! endpoints in [`fragments`], moderator chat in [`say`], the Blacklisted Words
//! tools in [`blacklist`], and the player tools in [`player`]. The view models
//! and markup they render through belong to `super::templates`.
//!
//! The session view's tools answer with JSON rather than a redirect, because that
//! page polls a live transcript every two seconds and navigating away would cost
//! the moderator their place. The Moderation Lists tools redirect as before; the
//! blacklist logic itself is shared between them, not duplicated.

mod blacklist;
mod fragments;
mod pages;
mod player;
mod say;

#[cfg(test)]
mod tests;

// The handlers keep their original `admin::chatmod::*` paths so the router in
// `main.rs` is untouched by the split. The submodules stay private: the
// `Form`/`Query` extractor types are named only inside a handler signature, so
// re-exporting them would widen the surface for nothing.
pub use blacklist::{blacklist_add, blacklist_remove, blacklist_toggle};
pub use fragments::{chatmod_data_fragment, chatmod_session_data};
pub use pages::{chatmod_audit_page, chatmod_lists_page, chatmod_page, chatmod_session_page};
pub use player::{chatmod_quick_blacklist, chatmod_warn};
pub use say::chatmod_say;
