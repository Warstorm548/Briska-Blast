//! The admin **Chat-Mod** tab — the chat-moderation panel.
//!
//! The renderers here consume view models the handler layer (`admin::chatmod`,
//! via `admin::chatmod_data`) projects from live Redis state. Two server-rendered
//! views share one full-width three-column shell — the landing page
//! (flagged-message overview) and the session view (transcript +
//! quick-access tools). This is the first tab to span the full
//! viewport width instead of the shared `.page` container; everything here is
//! `cm-`-prefixed so it can't collide with the common stylesheet.
//!
//! Split across one module per concern: [`model`] holds the view models,
//! [`style`] / [`script`] the page's CSS and JS, [`chrome`] the fixed furniture,
//! [`shell`] the three-column document every view renders into, and [`panels`] /
//! [`highlight`] the scrolling lists inside it. The four views live in [`pages`],
//! [`audit`], and [`lists`].

mod audit;
mod chrome;
mod highlight;
mod lists;
mod model;
mod pages;
mod panels;
mod script;
mod shell;
mod style;

pub use audit::chatmod_audit_page;
pub use lists::chatmod_lists_page;
pub use model::{
    AuditLog, BannedUser, BlacklistWord, ChatMessage, ChatSession, Deletion, FlaggedBody,
    FlaggedSession, ListAuditEntry, ModerationLists, PlayerAuditEntry, PreviewLine, SuspendedUser,
    SystemAuditEntry, WordAuditEntry,
};
pub use pages::{chatmod_landing_page, chatmod_session_page};
pub use panels::{
    chatmod_flagged_fragment, chatmod_sessions_fragment, chatmod_transcript_fragment,
};
