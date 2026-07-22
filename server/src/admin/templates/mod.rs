//! Server-rendered HTML for the admin panel — one submodule per page, plus a
//! `common` module for the shared escaper / stylesheet / nav bar. The page
//! renderers are re-exported here so callers keep using `templates::{...}`.

mod common;
mod dashboard;
mod login;
mod logs;
mod stats;
mod users;

pub use dashboard::{dashboard_page, DashboardData};
pub use login::{force_password_page, login_page, notice_page, LoginView};
pub use logs::logs_page;
pub use stats::stats_page;
pub use users::{users_page, UserRow};
