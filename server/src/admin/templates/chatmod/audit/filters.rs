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
pub(super) fn audit_filter_panel(
    cat: &str,
    actions: &[&str],
    specific: &str,
    range: &str,
    from: Option<&str>,
) -> String {
    let opts = actions
        .iter()
        .map(|a| format!("<option>{}</option>", escape(a)))
        .collect::<String>();
    // Carried through the submit so the range round-trip does not lose the
    // session a moderator arrived from — the X close and the Chat Nav links
    // both depend on it.
    let from_field = match from {
        Some(code) => format!(r#"<input type="hidden" name="from" value="{}">"#, escape(code)),
        None => String::new(),
    };
    format!(
        r#"<form class="cm-audit-filter" method="get" action="/admin/chatmod/audit">
      <p class="cm-audit-filter-title">Advanced Filter</p>
      {from_field}
      <input type="hidden" name="cat" value="{cat}">
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
          <label>Range<input type="text" name="range" value="{range}" placeholder="e.g. 200 or 100-200"></label>
        </fieldset>
        <fieldset class="cm-filter-group">
          <legend>This table</legend>
          {specific}
        </fieldset>
      </div>
      <div class="cm-filter-actions">
        <button type="submit" class="btn">Apply Range</button>
        <a href="/admin/chatmod/audit" class="btn">Reset</a>
      </div>
      <p class="note">Range is live. The other filters are preview only &mdash; not wired up yet.</p>
    </form>"#,
        cat = escape(cat),
        range = escape(range),
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
