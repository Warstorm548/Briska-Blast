//! The admin Dashboard page — stats, ports, version minimums, the update
//! system section, and change-password.

use super::common::{escape, nav_html, CSS};

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
