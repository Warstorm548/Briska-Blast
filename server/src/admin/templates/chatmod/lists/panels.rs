//! One panel per Moderation Lists sub-tab: Backlisted Words, Banned Users,
//! Active Suspensions, and the not-yet-designed Whitelisted Users placeholder.

use shared::types::player::PlayerId;

use super::super::super::common::escape;
use super::super::model::{BannedUser, BlacklistWord, SuspendedUser};

/// The **Backlisted Words** sub-tab: a three-column tools panel (add / remove /
/// CSV import) over the searchable "Words In List" ledger.
///
/// Add, Remove and the Active Filter toggle post for real. Delete routes through
/// the shared confirm modal, which collects the (logged-only) reason before
/// submitting. CSV import is still inert.
///
/// `from` threads the session a moderator came from through every post, so the
/// redirect lands back where they were rather than on the bare lists page.
pub(super) fn blacklist_panel_html(words: &[BlacklistWord], from: Option<&str>) -> String {
    let from_field = match from {
        Some(code) => format!(r#"<input type="hidden" name="from" value="{}">"#, escape(code)),
        None => String::new(),
    };
    let table = if words.is_empty() {
        r#"<p class="cm-empty">No blacklisted words.</p>"#.to_string()
    } else {
        let rows = words
            .iter()
            .map(|w| {
                let checked = if w.active_filter { " checked" } else { "" };
                // The toggle posts the value it is moving TO, so a stale page
                // can't flip a word the opposite way from what the moderator saw.
                let next = if w.active_filter { "0" } else { "1" };
                let word = escape(&w.word);
                format!(
                    r#"<tr>
            <td>{word}</td>
            <td>{reason}</td>
            <td class="cm-lists-check"><form method="post" action="/admin/chatmod/lists/blacklist/toggle" class="cm-inline-form">{from_field}<input type="hidden" name="words" value="{word}"><input type="hidden" name="active" value="{next}"><input type="checkbox"{checked} aria-label="Active filter for {word}" onchange="this.form.submit()"></form></td>
            <td class="cm-lists-check"><button type="button" class="btn-trash" title="Delete" aria-label="Delete {word}" data-word="{word}" onclick="bbCmListsDelete(this.getAttribute('data-word'))">&#128465;</button></td>
          </tr>"#,
                    reason = escape(&w.reason),
                )
            })
            .collect::<String>();
        format!(
            r#"<table class="cm-audit-table">
        <thead>
          <tr><th>Words</th><th>Reason Provided</th><th>Active Filter Toggle</th><th>Delete</th></tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
        )
    };
    format!(
        r#"<div class="cm-lists-tools">
  <form class="cm-lists-tool" method="post" action="/admin/chatmod/lists/blacklist/add">
    {from_field}
    <p class="cm-lists-tool-title">Add to Blacklist</p>
    <input type="text" class="cm-reason" name="reason" placeholder="Reason (logged)" aria-label="Reason for adding">
    <textarea name="words" placeholder="Word or words &mdash; separate with ;" aria-label="Words to blacklist"></textarea>
    <button type="submit" class="btn btn-sm">Confirm</button>
  </form>
  <form class="cm-lists-tool" method="post" action="/admin/chatmod/lists/blacklist/remove">
    {from_field}
    <p class="cm-lists-tool-title">Remove From Blacklist</p>
    <input type="text" class="cm-reason" name="reason" placeholder="Reason (logged)" aria-label="Reason for removing">
    <textarea name="words" placeholder="Word or words &mdash; separate with ;" aria-label="Words to remove"></textarea>
    <button type="submit" class="btn btn-sm">Confirm</button>
  </form>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Add Words From a CSV File</p>
    <label for="cm-lists-csv">Upload</label>
    <input type="file" id="cm-lists-csv" accept=".csv" aria-label="CSV file">
    <button type="button" class="btn btn-sm">Confirm</button>
    <p class="note">CSV import is not wired yet &mdash; paste words above for now.</p>
  </div>
</div>
<form method="post" action="/admin/chatmod/lists/blacklist/remove" id="cm-lists-del-form" hidden>
  {from_field}
  <input type="hidden" name="words" id="cm-lists-del-word">
  <input type="hidden" name="reason" id="cm-lists-del-reason">
</form>
<p class="cm-lists-note">More than one word can be added at once &mdash; separate each with a <span class="mono">;</span> so the system counts them individually. Each word occupies its own row in the list below.</p>
<div class="cm-lists-search">
  <label for="cm-lists-word-search">Words In List</label>
  <input type="text" id="cm-lists-word-search" placeholder="Search &mdash; most relevant words rise to the top">
</div>
<div class="cm-audit-scroll">
  {table}
</div>"#
    )
}

/// The **Banned Users** sub-tab: a *To Ban* / *Banned User Tools* panel over the
/// ban ledger. Ban and UnBan each open the shared confirm modal. Preview only.
pub(super) fn banned_panel_html(banned: &[BannedUser]) -> String {
    let table = if banned.is_empty() {
        r#"<p class="cm-empty">No banned users.</p>"#.to_string()
    } else {
        let rows = banned
            .iter()
            .map(|b| {
                let pid = PlayerId::from_counter(b.player_id);
                let transcript = if b.has_transcript {
                    r#"<button type="button" class="btn btn-sm">Transcript</button>"#.to_string()
                } else {
                    r#"<span class="cm-audit-none">&mdash;</span>"#.to_string()
                };
                let user = escape(&b.username);
                format!(
                    r#"<tr>
            <td class="cm-audit-ts mono">{ts}</td>
            <td>{user}</td>
            <td><span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">{pid}</span></td>
            <td>{reason}</td>
            <td>{transcript}</td>
            <td class="cm-lists-check"><input type="checkbox" aria-label="Select {user}"></td>
          </tr>"#,
                    ts = escape(&b.timestamp),
                    reason = escape(&b.reason),
                )
            })
            .collect::<String>();
        format!(
            r#"<table class="cm-audit-table">
        <thead>
          <tr><th>Timestamp</th><th>Username</th><th>User ID</th><th>Reason For Ban</th><th>Transcript</th><th>CheckBox</th></tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
        )
    };
    format!(
        r#"<div class="cm-lists-tools">
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">To Ban</p>
    <label for="cm-lists-ban-id">User ID</label>
    <input type="text" id="cm-lists-ban-id" placeholder="Player ID" aria-label="User ID to ban">
    <label for="cm-lists-ban-reason">Reason</label>
    <input type="text" id="cm-lists-ban-reason" placeholder="Reason (logged)">
    <label for="cm-lists-ban-words">Offensive Words</label>
    <input type="text" id="cm-lists-ban-words" placeholder="Word or words &mdash; separate with ;">
    <button type="button" class="btn btn-danger btn-sm" onclick="bbCmListsAsk('cm-lists-ban-modal')">Ban User</button>
  </div>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Banned User Tools</p>
    <p class="cm-lists-hint">To un-ban: tick the checkbox of each user you want to reinstate, then press UnBan. You'll confirm with a required reason.</p>
    <button type="button" class="btn btn-sm" onclick="bbCmListsAsk('cm-lists-unban-modal')">UnBan</button>
  </div>
</div>
<p class="cm-lists-title">Banned Users</p>
<div class="cm-audit-scroll">
  {table}
</div>"#
    )
}

/// The **Active Suspensions** sub-tab (the old standalone "Suspensions" nav item,
/// now folded in here): an *Extend / Clear / Suspend* tools panel over the
/// suspension ledger. Suspend-from-this-page is flagged under construction.
pub(super) fn suspensions_panel_html(suspended: &[SuspendedUser]) -> String {
    let table = if suspended.is_empty() {
        r#"<p class="cm-empty">No active suspensions.</p>"#.to_string()
    } else {
        let rows = suspended
            .iter()
            .map(|s| {
                let pid = PlayerId::from_counter(s.player_id);
                let user = escape(&s.username);
                format!(
                    r#"<tr>
            <td class="cm-audit-ts mono">{ts}</td>
            <td>{user}</td>
            <td><span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">{pid}</span></td>
            <td>{dur}</td>
            <td>{rem}</td>
            <td>{reason}</td>
            <td class="cm-lists-check"><input type="checkbox" aria-label="Select {user}"></td>
          </tr>"#,
                    ts = escape(&s.timestamp),
                    dur = escape(&s.suspended_for),
                    rem = escape(&s.remaining),
                    reason = escape(&s.reason),
                )
            })
            .collect::<String>();
        format!(
            r#"<table class="cm-audit-table">
        <thead>
          <tr><th>TimeStamp</th><th>Username</th><th>UserID</th><th>Suspended For</th><th>Remaining Time Left</th><th>Reason</th><th>CheckBox</th></tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
        )
    };
    format!(
        r#"<div class="cm-lists-tools">
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Extend Current Suspension</p>
    <div class="row">
      <input type="text" class="cm-dur" inputmode="numeric" placeholder="Days" aria-label="Days">
      <input type="text" class="cm-dur" inputmode="numeric" placeholder="Hours" aria-label="Hours">
      <input type="text" class="cm-dur" inputmode="numeric" placeholder="Mins" aria-label="Minutes">
    </div>
    <input type="text" class="cm-reason" placeholder="Reason (logged)">
    <button type="button" class="btn btn-sm">Extend</button>
  </div>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Clear Suspensions</p>
    <input type="text" class="cm-reason" placeholder="Reason (logged)">
    <button type="button" class="btn btn-sm">Clear</button>
  </div>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Suspend User</p>
    <input type="text" class="cm-reason" placeholder="Reason (logged)">
    <button type="button" class="btn btn-sm">Suspend</button>
    <p class="note">Suspending a user from this page is under construction.</p>
  </div>
</div>
<p class="cm-lists-title">Suspended Users</p>
<div class="cm-audit-scroll">
  {table}
</div>"#
    )
}

/// The **Whitelisted Users** sub-tab — no mockup yet, so a placeholder panel.
pub(super) fn whitelist_panel_html() -> String {
    r#"<div class="cm-lists-tools">
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Whitelisted Users</p>
    <p class="cm-lists-hint">Under construction &mdash; this list's layout lands in a later pass.</p>
  </div>
</div>
<p class="cm-empty">No whitelist design yet.</p>"#
        .to_string()
}
