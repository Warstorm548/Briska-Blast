//! The admin Users page — search, dev-flag toggles, and a delete-confirm modal.

use super::common::{escape, nav_html, CSS};
use crate::admin::AdminRole;

pub struct UserRow {
    pub id: String,
    pub username: String,
    pub dev_flag: bool,
}

pub fn users_page(
    users: &[UserRow],
    q: &str,
    field: &str,
    message: Option<(bool, String)>,
    role: AdminRole,
    username: &str,
) -> String {
    let nav = nav_html("users", role, username);
    let msg_html = match &message {
        Some((true, text)) => format!(r#"<div class="msg-ok">&#10003; {}</div>"#, escape(text)),
        Some((false, text)) => format!(r#"<div class="msg-err">&#10007; {}</div>"#, escape(text)),
        None => String::new(),
    };

    let field_username_sel = if field == "id" { "" } else { " selected" };
    let field_id_sel = if field == "id" { " selected" } else { "" };
    let q_attr = escape(q);
    let count = users.len();

    let rows_html = if users.is_empty() {
        r#"<tr><td colspan="4" class="empty">No users match.</td></tr>"#.to_string()
    } else {
        users
            .iter()
            .map(|u| {
                let chk = if u.dev_flag { " checked" } else { "" };
                let id_e = escape(&u.id);
                let name_e = escape(&u.username);
                // Trash button is type="button" so it never submits the
                // surrounding dev-flag form; it only opens the confirm modal.
                // id/username ride along as escaped data-attributes and are
                // read via dataset (never interpolated into a JS string), so a
                // hostile username can't break out of the markup. &#128465; = 🗑.
                format!(
                    r#"<tr><td class="mono">{id_e}</td><td>{name_e}</td><td><input type="checkbox" name="dev_{id_e}"{chk}></td><td><button type="button" class="btn-trash" data-id="{id_e}" data-username="{name_e}" onclick="askDelete(this)" title="Delete user" aria-label="Delete user">&#128465;</button></td></tr>"#
                )
            })
            .collect::<String>()
    };

    let known_ids = users
        .iter()
        .map(|u| u.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let known_ids_attr = escape(&known_ids);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Briska Blast — Admin Users</title>
  <style>{CSS}</style>
</head>
<body>
<div class="page">
  <div class="card">
    {nav}
    <div style="height:14px"></div>
    {msg_html}
    <div class="section">
      <p class="section-title">Users</p>
      <p class="section-sub">{count} player(s) on this server. Tick the Dev box to grant dev-channel access; untick to revoke.</p>
      <form method="GET" action="/admin/users" class="row" style="margin-bottom:14px">
        <input type="text" name="q" value="{q_attr}" placeholder="Search…">
        <select name="field">
          <option value="username"{field_username_sel}>by username</option>
          <option value="id"{field_id_sel}>by id</option>
        </select>
        <button type="submit" class="btn btn-sm">Search</button>
        <a href="/admin/users" class="btn btn-sm" style="text-decoration:none;display:inline-block">Clear</a>
      </form>
      <form method="POST" action="/admin/users/dev-flag">
        <input type="hidden" name="known_ids" value="{known_ids_attr}">
        <table class="user-table">
          <thead><tr><th>ID</th><th>Username</th><th>Dev</th><th>Delete</th></tr></thead>
          <tbody>{rows_html}</tbody>
        </table>
        <button type="submit" class="btn btn-primary" style="margin-top:14px">Confirm changes</button>
      </form>
    </div>
  </div>
</div>

<!-- Delete-confirm modal. Lives OUTSIDE the dev-flag form (nested forms are
     invalid HTML) and is position:fixed, so the full-viewport backdrop covers
     and blocks the page until Cancel or Delete is chosen. -->
<div id="del-backdrop" class="modal-backdrop">
  <div class="modal-card" role="dialog" aria-modal="true" aria-labelledby="del-title">
    <p class="section-title" id="del-title">Delete user?</p>
    <p class="section-sub mono" id="del-msg"></p>
    <p class="section-sub">Frees the ID number for reuse and removes its secret + username. This cannot be undone.</p>
    <form method="POST" action="/admin/users/delete" class="modal-actions">
      <input type="hidden" name="id" id="del-id">
      <button type="button" class="btn btn-sm" onclick="closeDelete()">Cancel</button>
      <button type="submit" class="btn btn-danger btn-sm">Delete</button>
    </form>
  </div>
</div>
<script>
  function askDelete(btn) {{
    var id = btn.dataset.id, name = btn.dataset.username;
    document.getElementById('del-id').value = id;
    // textContent (not innerHTML) so the username renders literally.
    document.getElementById('del-msg').textContent = id + ' (' + name + ')';
    document.getElementById('del-backdrop').style.display = 'flex';
  }}
  function closeDelete() {{
    // Cancel: hide the modal and clear the target id. Nothing is submitted, so
    // no deletion happens — only the Delete button POSTs.
    document.getElementById('del-backdrop').style.display = 'none';
    document.getElementById('del-id').value = '';
  }}
  document.addEventListener('keydown', function (e) {{
    if (e.key === 'Escape') closeDelete();
  }});
</script>
</body>
</html>
"#
    )
}
