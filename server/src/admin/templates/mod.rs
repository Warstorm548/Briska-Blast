//! Server-rendered HTML for the admin panel — one submodule per page, plus a
//! `common` module for the shared escaper / stylesheet / nav bar. The page
//! renderers are re-exported here so callers keep using `templates::{...}`.

mod chatmod;
mod common;
mod dashboard;
mod login;
mod logs;
mod stats;
mod users;

pub use chatmod::{
    chatmod_audit_page, chatmod_flagged_fragment, chatmod_landing_page, chatmod_lists_page,
    chatmod_session_page, chatmod_sessions_fragment, chatmod_transcript_fragment, AuditLog,
    BannedUser, BlacklistWord, ChatMessage, ChatSession, Deletion, FlaggedBody, FlaggedSession,
    ListAuditEntry, ModerationLists, PlayerAuditEntry, PreviewLine, SuspendedUser,
    SystemAuditEntry, WordAuditEntry,
};
pub use dashboard::{dashboard_page, DashboardData};
pub use login::{force_password_page, login_page, notice_page, LoginView};
pub use logs::logs_page;
pub use stats::stats_page;
pub use users::{users_page, UserRow};
