//! Chat capture and moderation data — everything behind the admin **Chat-Mod**
//! tab that isn't markup.
//!
//! The game already relays every lobby/session chat message through the server
//! (`ClientMsg::SendChat` → `ServerMsg::ChatMessage`, handled in
//! `signaling/ws/frame.rs`). This module hangs off that existing path: each
//! accepted message is given a body id ([`ids`]), matched against the blacklist
//! ([`blacklist`]), and appended to a transcript ([`transcript`]) that moderators
//! watch live. Moderation events land in [`audit`].
//!
//! # Storage
//!
//! Redis, using the same patterns as the rest of the server — plain commands, no
//! Lua round-tripping of structs, so the lua-cjson empty-array pitfall documented
//! in `api/mod.rs` does not apply here.
//!
//! **No key in this module carries a TTL.** `session:{CODE}` is the only key in
//! the deployment that expires; chat data lives and dies by explicit teardown.
//! That is a deliberate choice, and it means every path that ends a session must
//! route through [`transcript::on_session_end`] — plus the orphan sweep, for
//! sessions that vanish by passive TTL expiry with no server code running.
//!
//! # Retention
//!
//! A transcript is written from a session's first message so a moderator has
//! something live to watch, but survives only when something makes it evidence:
//!
//! | Trigger | Outcome |
//! |---|---|
//! | Session ends clean | transcript deleted outright, unrecoverable |
//! | Blacklisted word detected | retained permanently + a System audit record |
//! | Moderator acts, or types a single message | retained permanently |
//!
//! Retention is retroactive: it keeps the *whole* conversation, not just the part
//! after the trigger, which is the point of capturing from the first message.
//!
//! A "snapshot of the chat as it stood" is not a copy. The transcript is stored
//! once and each audit record pins a cut index plus the body ids it covered;
//! rendering replays the transcript up to that cut. This is only sound because
//! the transcript is append-only — a future Delete Body must write a tombstone
//! rather than removing the entry, or older snapshots stop being faithful.

pub mod audit;
pub mod blacklist;
pub mod ids;
pub mod transcript;
