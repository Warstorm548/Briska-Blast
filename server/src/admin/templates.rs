pub struct DashboardData {
    pub min_launcher_version: String,
    pub min_game_version: String,
    pub game_port: u16,
    pub admin_port: u16,
    pub session_count: usize,
    pub player_count: u64,
    pub message: Option<(bool, String)>,
    pub using_default_password: bool,
    // update system
    pub release_channel: &'static str,
    pub server_version: &'static str,
    pub update_last_checked: Option<String>,
    pub update_available: Option<String>,
    pub update_auto_enabled: bool,
    pub update_check_interval_secs: u64,
    pub update_apply_interval_secs: Option<u64>,
    pub update_scheduled_at: Option<String>,
    pub update_scheduled_version: Option<String>,
    pub update_previous_version: Option<String>,
    pub update_rollback_locked: bool,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const CSS: &str = "
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: system-ui, -apple-system, sans-serif; background: #0d1117; color: #c9d1d9; min-height: 100vh; padding: 24px 16px; }
.page { max-width: 560px; margin: 0 auto; }
.brand { color: #e94560; font-size: 1.1rem; font-weight: 700; letter-spacing: 0.5px; }
.card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 28px; }
nav { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0; padding-bottom: 18px; border-bottom: 1px solid #30363d; }
.section { padding: 20px 0; border-bottom: 1px solid #30363d; }
.section:last-child { border-bottom: none; padding-bottom: 4px; }
.section-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; margin-bottom: 4px; }
.section-sub { font-size: 0.82rem; color: #8b949e; margin-bottom: 14px; }
.field { margin-bottom: 12px; }
.field:last-child { margin-bottom: 0; }
label { display: block; font-size: 0.8rem; color: #8b949e; margin-bottom: 4px; }
.current { font-size: 0.75rem; color: #6e7681; margin-bottom: 6px; }
.row { display: flex; gap: 8px; align-items: center; }
input[type=text], input[type=password] { flex: 1; background: #0d1117; border: 1px solid #30363d; border-radius: 6px; color: #c9d1d9; padding: 7px 10px; font-size: 0.875rem; outline: none; width: 100%; }
input:focus { border-color: #388bfd; }
.btn { background: #21262d; border: 1px solid #30363d; border-radius: 6px; color: #c9d1d9; padding: 7px 14px; font-size: 0.875rem; cursor: pointer; white-space: nowrap; }
.btn:hover { background: #30363d; }
.btn-primary { background: #e94560; border-color: #b5243d; color: #fff; }
.btn-primary:hover { background: #c73652; }
.btn-sm { padding: 6px 12px; font-size: 0.8rem; }
.stats { display: flex; gap: 12px; }
.stat-box { flex: 1; background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 14px; text-align: center; }
.stat-num { font-size: 1.8rem; font-weight: 700; color: #e94560; line-height: 1; }
.stat-lbl { font-size: 0.72rem; color: #6e7681; margin-top: 4px; }
.msg-ok  { background: #0f2912; border: 1px solid #238636; border-radius: 6px; color: #3fb950; padding: 9px 13px; font-size: 0.83rem; margin-bottom: 14px; }
.msg-err { background: #2d1117; border: 1px solid #da3633; border-radius: 6px; color: #f85149; padding: 9px 13px; font-size: 0.83rem; margin-bottom: 14px; }
.warn    { background: #2d2308; border: 1px solid #d29922; border-radius: 6px; color: #e3b341; padding: 9px 13px; font-size: 0.82rem; margin-bottom: 14px; }
.note    { font-size: 0.74rem; color: #6e7681; margin-top: 6px; }
.toggle-wrap { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
.toggle { position: relative; display: inline-block; width: 38px; height: 22px; }
.toggle input { opacity: 0; width: 0; height: 0; }
.slider { position: absolute; cursor: pointer; inset: 0; background: #30363d; border-radius: 22px; transition: .2s; }
.slider:before { position: absolute; content: ''; height: 16px; width: 16px; left: 3px; bottom: 3px; background: #8b949e; border-radius: 50%; transition: .2s; }
input:checked + .slider { background: #e94560; }
input:checked + .slider:before { transform: translateX(16px); background: #fff; }
.toggle-label { font-size: 0.85rem; color: #c9d1d9; }
.update-meta { font-size: 0.75rem; color: #6e7681; margin-bottom: 14px; }
.update-found { background: #0f2912; border: 1px solid #238636; border-radius: 6px; color: #3fb950; padding: 10px 13px; font-size: 0.83rem; margin-bottom: 12px; }
.update-sched { background: #0d1f36; border: 1px solid #388bfd; border-radius: 6px; color: #79c0ff; padding: 10px 13px; font-size: 0.83rem; margin-bottom: 12px; display: flex; justify-content: space-between; align-items: center; }
.rollback-box { background: #2d1117; border: 1px solid #da3633; border-radius: 6px; color: #f85149; padding: 10px 13px; font-size: 0.83rem; margin-bottom: 12px; display: flex; justify-content: space-between; align-items: center; }
.rollback-locked { background: #2d2308; border: 1px solid #d29922; border-radius: 6px; color: #e3b341; padding: 9px 13px; font-size: 0.82rem; margin-bottom: 12px; }
.auto-settings { margin-left: 4px; margin-bottom: 4px; }
select { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; color: #c9d1d9; padding: 6px 8px; font-size: 0.875rem; }
input[type=datetime-local] { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; color: #c9d1d9; padding: 6px 8px; font-size: 0.875rem; }
.nav-left { display: flex; align-items: center; gap: 18px; }
.nav-link { color: #8b949e; text-decoration: none; font-size: 0.85rem; padding-bottom: 2px; border-bottom: 2px solid transparent; }
.nav-link:hover { color: #c9d1d9; }
.nav-link-active { color: #e94560; border-bottom-color: #e94560; }
.user-table { width: 100%; border-collapse: collapse; }
.user-table th, .user-table td { padding: 8px 6px; text-align: left; border-bottom: 1px solid #21262d; font-size: 0.85rem; }
.user-table th { color: #8b949e; font-weight: 600; font-size: 0.72rem; text-transform: uppercase; letter-spacing: 1px; }
.user-table td.empty { color: #6e7681; text-align: center; padding: 18px 6px; }
.mono { font-family: ui-monospace, SFMono-Regular, monospace; }
.btn-danger { background: #da3633; border-color: #b5243d; color: #fff; }
.btn-danger:hover { background: #f85149; }
.btn-trash { background: none; border: none; color: #8b949e; cursor: pointer; font-size: 1rem; line-height: 1; padding: 4px 8px; border-radius: 6px; }
.btn-trash:hover { color: #f85149; background: #2d1117; }
.modal-backdrop { display: none; position: fixed; inset: 0; background: rgba(1,4,9,0.7); z-index: 1000; align-items: center; justify-content: center; padding: 24px 16px; }
.modal-card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 24px; width: 100%; max-width: 380px; }
.modal-actions { display: flex; gap: 8px; justify-content: flex-end; margin: 18px 0 0; }
";

fn nav_html(active: &str) -> String {
    let dash_active = if active == "dashboard" { " nav-link-active" } else { "" };
    let users_active = if active == "users" { " nav-link-active" } else { "" };
    format!(
        r#"<nav>
  <div class="nav-left">
    <span class="brand">Briska Blast Admin</span>
    <a href="/admin/dashboard" class="nav-link{dash_active}">Dashboard</a>
    <a href="/admin/users" class="nav-link{users_active}">Users</a>
  </div>
  <form method="POST" action="/admin/logout" style="margin:0">
    <button type="submit" class="btn btn-sm">Logout</button>
  </form>
</nav>"#
    )
}

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
) -> String {
    let nav = nav_html("users");
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

pub fn login_page(error: Option<&str>) -> String {
    let err_html = error
        .map(|e| format!(r#"<div class="msg-err">{}</div>"#, escape(e)))
        .unwrap_or_default();
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Briska Blast — Admin</title>
  <style>{CSS}</style>
</head>
<body>
  <div style="display:flex;align-items:center;justify-content:center;min-height:100vh;padding:24px 16px">
    <div class="card" style="width:100%;max-width:360px">
      <div class="brand" style="margin-bottom:2px">Briska Blast</div>
      <p style="font-size:0.85rem;color:#6e7681;margin-bottom:24px">Admin Panel</p>
      {err_html}
      <form method="POST" action="/admin/login">
        <div class="field" style="margin-bottom:14px">
          <label for="pw">Password</label>
          <input type="password" id="pw" name="password" autofocus required>
        </div>
        <button type="submit" class="btn btn-primary" style="width:100%">Login</button>
      </form>
    </div>
  </div>
</body>
</html>"#
    )
}

fn build_update_section(data: &DashboardData) -> String {
    let channel = escape(data.release_channel);
    let version = escape(data.server_version);
    let last_checked = data.update_last_checked.as_deref().map(escape).unwrap_or_else(|| "Never".to_string());

    let rollback_locked_html = if data.update_rollback_locked {
        r#"<div class="rollback-locked">&#9888; Auto-update was disabled after a rollback. Re-enable it above once you have verified the rolled-back version is stable.</div>"#.to_string()
    } else {
        String::new()
    };

    let scheduled_html = if let Some(sched_at) = &data.update_scheduled_at {
        let version_label = data.update_scheduled_version.as_deref().unwrap_or("update");
        format!(
            r#"<div class="update-sched">
              <span>&#128197; <strong>{}</strong> scheduled for <strong>{}</strong></span>
              <form method="POST" action="/admin/update/cancel" style="margin:0">
                <button type="submit" class="btn btn-sm">Cancel</button>
              </form>
            </div>"#,
            escape(version_label), escape(sched_at)
        )
    } else {
        String::new()
    };

    let update_found_html = if let Some(ref available) = data.update_available {
        if data.update_scheduled_at.is_none() {
            format!(
                r#"<div class="update-found">
                  &#8593; Update available: <strong>{}</strong>
                  <div style="display:flex;gap:8px;margin-top:10px;flex-wrap:wrap">
                    <form method="POST" action="/admin/update/apply-now" style="margin:0">
                      <button type="submit" class="btn btn-primary btn-sm">Apply Now</button>
                    </form>
                    <form method="POST" action="/admin/update/schedule" style="margin:0;display:flex;gap:6px;align-items:center">
                      <input type="datetime-local" name="scheduled_at" required title="Enter time in UTC">
                      <span style="font-size:0.72rem;color:#6e7681">UTC</span>
                      <button type="submit" class="btn btn-sm">Schedule</button>
                    </form>
                  </div>
                  <div class="note">Scheduled time is interpreted as UTC — convert from your local timezone before entering.</div>
                </div>"#,
                escape(available)
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let rollback_html = if let Some(ref prev) = data.update_previous_version {
        if data.update_scheduled_at.is_none() {
            format!(
                r#"<div class="rollback-box">
                  <span>&#8595; Previous version: <strong>{}</strong></span>
                  <form method="POST" action="/admin/update/rollback" style="margin:0">
                    <input type="hidden" name="version" value="{}">
                    <button type="submit" class="btn btn-sm" style="border-color:#da3633;color:#f85149">Rollback to {}</button>
                  </form>
                </div>"#,
                escape(prev), escape(prev), escape(prev)
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let auto_checked = if data.update_auto_enabled { "checked" } else { "" };

    let check_interval_options = [("21600","6 hours"),("43200","12 hours"),("86400","24 hours"),("172800","48 hours")];
    let check_opts: String = check_interval_options.iter().map(|(val, label)| {
        let sel = if data.update_check_interval_secs == val.parse::<u64>().unwrap_or(0) { "selected" } else { "" };
        format!(r#"<option value="{}" {}>{}</option>"#, val, sel, label)
    }).collect();

    let apply_interval_options = [("0","Immediately"),("86400","1 day"),("259200","3 days"),("604800","1 week"),("1209600","2 weeks")];
    let apply_opts: String = apply_interval_options.iter().map(|(val, label)| {
        let sel = if data.update_apply_interval_secs.unwrap_or(0) == val.parse::<u64>().unwrap_or(0) { "selected" } else { "" };
        format!(r#"<option value="{}" {}>{}</option>"#, val, sel, label)
    }).collect();

    let auto_settings_html = if data.update_auto_enabled {
        format!(
            r#"<div class="auto-settings" id="auto-settings">
              <form method="POST" action="/admin/update/settings">
                <input type="hidden" name="auto_enabled" value="on">
                <div class="field" style="margin-bottom:10px">
                  <label>Check every</label>
                  <select name="check_interval_secs" onchange="this.form.submit()">{}</select>
                </div>
                <div class="field">
                  <label>Apply after</label>
                  <select name="apply_interval_secs" onchange="this.form.submit()">{}</select>
                </div>
              </form>
            </div>"#,
            check_opts, apply_opts
        )
    } else {
        String::new()
    };

    format!(
        r#"<div class="section">
      <p class="section-title">Server Updates</p>
      <p class="update-meta">Channel: <strong>{channel}</strong> &nbsp;|&nbsp; Version: <strong>v{version}</strong> &nbsp;|&nbsp; Last checked: {last_checked}</p>

      {rollback_locked_html}
      {scheduled_html}
      {update_found_html}
      {rollback_html}

      <form method="POST" action="/admin/update/check" style="margin-bottom:14px">
        <button type="submit" class="btn btn-sm">Check for Updates</button>
      </form>

      <div class="toggle-wrap">
        <form method="POST" action="/admin/update/settings" style="margin:0">
          <label class="toggle">
            <input type="checkbox" name="auto_enabled" value="on" {auto_checked} onchange="this.form.submit()">
            <span class="slider"></span>
          </label>
        </form>
        <span class="toggle-label">Enable Automatic Updates</span>
      </div>
      {auto_settings_html}
    </div>"#
    )
}

pub fn dashboard_page(data: &DashboardData) -> String {
    let msg_html = match &data.message {
        Some((true, text)) => format!(r#"<div class="msg-ok">&#10003; {}</div>"#, escape(text)),
        Some((false, text)) => format!(r#"<div class="msg-err">&#10007; {}</div>"#, escape(text)),
        None => String::new(),
    };

    let default_pw_warn = if data.using_default_password {
        r#"<div class="warn">&#9888; You are using the default password. Change it now in the section below.</div>"#.to_string()
    } else {
        String::new()
    };

    let nav = nav_html("dashboard");
    let min_launcher = escape(&data.min_launcher_version);
    let min_game = escape(&data.min_game_version);
    let game_port = data.game_port;
    let admin_port = data.admin_port;
    let sessions = data.session_count;
    let players = data.player_count;
    let update_section = build_update_section(data);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Briska Blast — Admin</title>
  <style>{CSS}</style>
</head>
<body>
<div class="page">
  <div class="card">
    {nav}

    <div style="height:14px"></div>
    {default_pw_warn}{msg_html}

    <div class="section">
      <p class="section-title">Server Stats</p>
      <div class="stats" style="margin-top:12px">
        <div class="stat-box">
          <div class="stat-num">{sessions}</div>
          <div class="stat-lbl">Active Sessions</div>
        </div>
        <div class="stat-box">
          <div class="stat-num">{players}</div>
          <div class="stat-lbl">Total Players</div>
        </div>
      </div>
    </div>

    <div class="section">
      <p class="section-title">Server Ports</p>
      <p class="section-sub">Fixed at startup — change via environment variable and restart.</p>
      <div class="stats" style="margin-top:12px">
        <div class="stat-box">
          <div class="stat-num" style="font-size:1.2rem">{game_port}</div>
          <div class="stat-lbl">Game Port (GAME_PORT)</div>
        </div>
        <div class="stat-box">
          <div class="stat-num" style="font-size:1.2rem">{admin_port}</div>
          <div class="stat-lbl">Admin Port (ADMIN_PORT)</div>
        </div>
      </div>
    </div>

    <div class="section">
      <p class="section-title">Version Control</p>
      <p class="section-sub">Version Minimums to Join Game Sessions</p>

      <div class="field">
        <label>Launcher Version</label>
        <p class="current">Currently enforcing: {min_launcher}</p>
        <form method="POST" action="/admin/update/launcher-version">
          <div class="row">
            <input type="text" name="version" placeholder="e.g. 1.2.0" required>
            <button type="submit" class="btn btn-sm">Update</button>
          </div>
        </form>
      </div>

      <div class="field">
        <label>Game Version</label>
        <p class="current">Currently enforcing: {min_game}</p>
        <form method="POST" action="/admin/update/game-version">
          <div class="row">
            <input type="text" name="version" placeholder="e.g. 1.2.0" required>
            <button type="submit" class="btn btn-sm">Update</button>
          </div>
        </form>
      </div>
    </div>

    {update_section}

    <div class="section">
      <p class="section-title">Change Password</p>
      <form method="POST" action="/admin/update/password" style="margin-top:12px">
        <div class="field">
          <label>Current Password</label>
          <input type="password" name="current_password" required>
        </div>
        <div class="field">
          <label>New Password</label>
          <input type="password" name="new_password" required>
        </div>
        <div class="field" style="margin-bottom:16px">
          <label>Confirm New Password</label>
          <input type="password" name="confirm_password" required>
        </div>
        <button type="submit" class="btn btn-primary">Update Password</button>
      </form>
    </div>

  </div>
</div>
</body>
</html>"#
    )
}
