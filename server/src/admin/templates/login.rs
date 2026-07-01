//! The admin login page — the one authenticated-flow page without a nav bar.

use super::common::{escape, CSS};

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
