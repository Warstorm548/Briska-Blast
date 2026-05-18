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
    pub _update_manual_override: bool,
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
