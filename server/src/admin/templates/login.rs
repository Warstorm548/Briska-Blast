//! The admin login page — the one authenticated-flow page without a nav bar.
//! Driven by [`LoginView`] so the same renderer covers all cases: OIDC-only,
//! password-only (fail-open / break-glass), and OIDC + emergency password.

use super::common::{escape, CSS};

/// What the login page should show. `banner`/`error` share a lifetime because
/// both are transient borrows built at the call site.
pub struct LoginView<'a> {
    /// Render the "Sign in with Pocket ID" button.
    pub oidc_enabled: bool,
    /// Render the emergency password form.
    pub show_password: bool,
    /// Optional info banner (e.g. "Pocket ID unreachable" / break-glass note).
    pub banner: Option<&'a str>,
    /// Optional error (e.g. a failed password attempt).
    pub error: Option<&'a str>,
}

pub fn login_page(v: &LoginView) -> String {
    let banner_html = v
        .banner
        .map(|b| format!(r#"<div class="warn">{}</div>"#, escape(b)))
        .unwrap_or_default();
    let err_html = v
        .error
        .map(|e| format!(r#"<div class="msg-err">{}</div>"#, escape(e)))
        .unwrap_or_default();

    let oidc_html = if v.oidc_enabled {
        r#"<a href="/admin/oidc/login" class="btn btn-primary" style="width:100%;display:block;text-align:center;text-decoration:none;box-sizing:border-box">Sign in with Pocket ID</a>"#.to_string()
    } else {
        String::new()
    };

    let divider_html = if v.oidc_enabled && v.show_password {
        r#"<div style="display:flex;align-items:center;gap:10px;margin:18px 0;color:#6e7681;font-size:0.75rem"><span style="flex:1;height:1px;background:#30363d"></span>OR<span style="flex:1;height:1px;background:#30363d"></span></div>"#.to_string()
    } else {
        String::new()
    };

    let password_html = if v.show_password {
        r#"<form method="POST" action="/admin/login">
        <div class="field" style="margin-bottom:14px">
          <label for="pw">Password</label>
          <input type="password" id="pw" name="password" autofocus required>
        </div>
        <button type="submit" class="btn btn-primary" style="width:100%">Login</button>
      </form>"#
            .to_string()
    } else {
        String::new()
    };

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
      {banner_html}{err_html}
      {oidc_html}
      {divider_html}
      {password_html}
    </div>
  </div>
</body>
</html>"#
    )
}

/// A standalone message page (no nav) — used by the OIDC callback for denied /
/// error / expired outcomes, with a link back to the login page.
pub fn notice_page(title: &str, message: &str) -> String {
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
    <div class="card" style="width:100%;max-width:400px">
      <div class="brand" style="margin-bottom:12px">Briska Blast</div>
      <p class="section-title" style="margin-bottom:6px">{title}</p>
      <p class="section-sub" style="margin-bottom:18px">{message}</p>
      <a href="/admin" class="btn btn-sm" style="text-decoration:none;display:inline-block">Back to sign in</a>
    </div>
  </div>
</body>
</html>"#,
        title = escape(title),
        message = escape(message),
    )
}
