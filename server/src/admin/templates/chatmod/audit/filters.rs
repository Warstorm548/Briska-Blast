//! The Advanced Filter panel above each category table, plus the per-category
//! fieldset bodies it wraps. Inert placeholders — filtering is not wired yet.

use super::super::super::common::escape;

/// The per-table Advanced Filter panel: a persistent group (the shared spine —
/// carries across tables) beside a table-specific group. `actions` populates the
/// persistent Action dropdown with that table's actions; `specific` is the
/// table-specific fieldset body. Inert placeholders — not wired.
///
/// The From/To time pickers carry `lang="en-GB"` so the native control renders a
/// 24-hour clock (matching the `… UTC` timestamps) despite the page's `en`
/// (12-hour) locale. The eventual per-moderator setting owns 12/24h + timezone.
pub(super) fn audit_filter_panel(actions: &[&str], specific: &str) -> String {
    let opts = actions
        .iter()
        .map(|a| format!("<option>{}</option>", escape(a)))
        .collect::<String>();
    format!(
        r#"<div class="cm-audit-filter">
      <p class="cm-audit-filter-title">Advanced Filter</p>
      <div class="cm-filter-grid">
        <fieldset class="cm-filter-group">
          <legend>Applies to all logs</legend>
          <label>From date<input type="date"></label>
          <label>To date<input type="date"></label>
          <label>From time (UTC)<input type="time" lang="en-GB"></label>
          <label>To time (UTC)<input type="time" lang="en-GB"></label>
          <label>Moderator<input type="text" placeholder="Display name"></label>
          <label>Group<select><option>Any</option><option>Admin</option><option>Moderator</option><option>Superadmin</option><option>System</option></select></label>
          <label>Action<select><option>Any</option>{opts}</select></label>
          <label>Reason<input type="text" placeholder="Search reason"></label>
        </fieldset>
        <fieldset class="cm-filter-group">
          <legend>This table</legend>
          {specific}
        </fieldset>
      </div>
      <p class="note">Preview only &mdash; filters are not wired up yet.</p>
    </div>"#
    )
}

/// Table-specific Advanced Filter fieldset bodies (inert placeholders).
pub(super) const PLAYER_FILTER_SPECIFIC: &str = r#"<label>Player<input type="text" placeholder="Username or ID"></label>
          <label>Body ID<input type="text" placeholder="Body ID"></label>
          <label>Flagged word<input type="text" placeholder="Word"></label>"#;

pub(super) const WORD_FILTER_SPECIFIC: &str = r#"<label>Word<input type="text" placeholder="Word"></label>
          <label>Player<input type="text" placeholder="Username or ID (Approve)"></label>"#;

pub(super) const LIST_FILTER_SPECIFIC: &str = r#"<label>Player<input type="text" placeholder="Username or ID"></label>
          <label>List<select><option>Any</option><option>Ban List</option><option>Suspensions</option><option>Whitelist</option></select></label>"#;

pub(super) const SYSTEM_FILTER_SPECIFIC: &str = r#"<label>Word<input type="text" placeholder="Word"></label>
          <label>Player<input type="text" placeholder="Username or ID"></label>
          <label>Source<input type="text" placeholder="Process (e.g. Word Filter)"></label>"#;
