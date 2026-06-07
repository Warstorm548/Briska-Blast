//! Per-feature `update` handlers. Each `Message` arm in `super::update`
//! delegates to one `pub(crate) fn handler(state, …) -> Task<Message>` here,
//! grouped by feature domain. Shared helpers (`register_tasks`,
//! `recompute_branch_updates_available`, `launch_game`, …) live in `super`
//! and are called via `crate::app::…`.

pub(crate) mod firewall;
pub(crate) mod identity;
pub(crate) mod install;
pub(crate) mod launcher_update;
pub(crate) mod maintenance;
pub(crate) mod nav;
pub(crate) mod play;
