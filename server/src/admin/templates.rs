pub struct DashboardData {
    pub min_launcher_version: String,
    pub min_game_version: String,
    pub active_bind_addr: String,
    pub saved_bind_addr: String,
    pub session_count: usize,
    pub player_count: u64,
    pub message: Option<(bool, String)>,
    pub using_default_password: bool,
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
";

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

    let bind_note = if data.active_bind_addr != data.saved_bind_addr {
        format!(
            r#"<p class="note">&#9888; Pending restart &mdash; will change to <strong>{}</strong> on next restart via Portainer.</p>"#,
            escape(&data.saved_bind_addr)
        )
    } else {
        String::new()
    };

    let min_launcher = escape(&data.min_launcher_version);
    let min_game = escape(&data.min_game_version);
    let active_bind = escape(&data.active_bind_addr);
    let saved_bind = escape(&data.saved_bind_addr);
    let sessions = data.session_count;
    let players = data.player_count;

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
    <nav>
      <span class="brand">Briska Blast Admin</span>
      <form method="POST" action="/admin/logout" style="margin:0">
        <button type="submit" class="btn btn-sm">Logout</button>
      </form>
    </nav>

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

    <div class="section">
      <p class="section-title">Server Bind Address</p>
      <p class="current" style="margin-top:10px">Active now: {active_bind}</p>
      <p class="current">Saved: {saved_bind}</p>
      {bind_note}
      <form method="POST" action="/admin/update/bind-addr" style="margin-top:10px">
        <div class="row">
          <input type="text" name="bind_addr" placeholder="e.g. 0.0.0.0:8080" value="{saved_bind}" required>
          <button type="submit" class="btn btn-sm">Save</button>
        </div>
      </form>
      <p class="note">Takes effect after restarting the container in Portainer.</p>
    </div>

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
