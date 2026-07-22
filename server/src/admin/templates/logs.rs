//! The admin **Logs** tab — source/line selectors, client-side level + text
//! filters, auto-refresh polling, and a Download button. Every line shown here
//! was already IP-redacted server-side (`admin::logs`).

use super::common::{escape, nav_html, CSS};
use crate::admin::AdminRole;

/// `sources` is (key, label) for every source this role may read; `current` is
/// the selected key. `body` is the initial, already-redacted tail.
#[allow(clippy::too_many_arguments)]
pub fn logs_page(
    sources: &[(&str, &str)],
    current: &str,
    lines: u32,
    line_choices: &[u32],
    body: &str,
    error: Option<&str>,
    role: AdminRole,
    username: &str,
) -> String {
    let nav = nav_html("logs", role, username);

    let source_opts = sources
        .iter()
        .map(|(key, label)| {
            let sel = if *key == current { " selected" } else { "" };
            format!(
                r#"<option value="{}"{sel}>{}</option>"#,
                escape(key),
                escape(label)
            )
        })
        .collect::<String>();

    let line_opts = line_choices
        .iter()
        .map(|n| {
            let sel = if *n == lines { " selected" } else { "" };
            format!(r#"<option value="{n}"{sel}>{n} lines</option>"#)
        })
        .collect::<String>();

    let err_html = error
        .map(|e| format!(r#"<div class="msg-err">&#10007; {}</div>"#, escape(e)))
        .unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Briska Blast — Admin Logs</title>
  <style>{CSS}
  .log-controls {{ display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin-bottom: 12px; }}
  .log-controls .grow {{ flex: 1; min-width: 120px; }}
  .log-box {{ background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 12px; margin: 0; font-family: ui-monospace, SFMono-Regular, monospace; font-size: 0.78rem; line-height: 1.5; color: #c9d1d9; white-space: pre; overflow: auto; max-height: 64vh; }}
  .log-box.empty::before {{ content: 'No log lines.'; color: #6e7681; }}
  .auto-lbl {{ display: flex; align-items: center; gap: 6px; font-size: 0.8rem; color: #8b949e; white-space: nowrap; }}
  </style>
</head>
<body>
<div class="page">
  <div class="card">
    {nav}
    <div style="height:14px"></div>
    {err_html}
    <div class="section">
      <p class="section-title">Container Logs</p>
      <p class="section-sub">Live container output from this deployment. Client IPs are redacted to <span class="mono">&#10216;ip:&hellip;&#10217;</span> tokens before display.</p>
      <div class="log-controls">
        <select id="src">{source_opts}</select>
        <select id="lines">{line_opts}</select>
        <select id="level">
          <option value="all">All levels</option>
          <option value="INFO">Info+</option>
          <option value="WARN">Warn+</option>
          <option value="ERROR">Error</option>
        </select>
        <input type="text" id="filter" class="grow" placeholder="Filter text&hellip;">
        <button type="button" class="btn btn-sm" onclick="bbLogsLoad()">&#8635; Refresh</button>
        <label class="auto-lbl"><input type="checkbox" id="auto" onchange="bbLogsAuto()"> Auto (5s)</label>
        <a id="dl" class="btn btn-sm" style="text-decoration:none" href="/admin/logs">Download .log</a>
      </div>
      <pre id="logbox" class="log-box">{body_html}</pre>
    </div>
  </div>
</div>
<script>
(function(){{
  var box = document.getElementById('logbox');
  // Seed from the server-rendered tail via textContent — never interpolate log
  // text into JS. All filtering works on this already-redacted buffer.
  var rawLog = box.textContent;
  function qs() {{
    return 'source=' + encodeURIComponent(document.getElementById('src').value)
         + '&lines=' + encodeURIComponent(document.getElementById('lines').value);
  }}
  function render() {{
    var level = document.getElementById('level').value;
    var needle = document.getElementById('filter').value.toLowerCase();
    var order = {{ 'INFO': 1, 'WARN': 2, 'ERROR': 3 }};
    var min = order[level] || 0;
    var out = rawLog.split('\n').filter(function(line) {{
      if (needle && line.toLowerCase().indexOf(needle) === -1) return false;
      if (min === 0) return true;
      var up = line.toUpperCase();
      // Keep the line if it carries a level at or above the selected floor.
      for (var k in order) {{ if (order[k] >= min && up.indexOf(k) !== -1) return true; }}
      return false;
    }}).join('\n');
    box.textContent = out;
    box.classList.toggle('empty', out.trim() === '');
  }}
  window.bbLogsLoad = function() {{
    document.getElementById('dl').href = '/admin/logs/download?' + qs();
    fetch('/admin/logs/data?' + qs()).then(function(r) {{
      if (r.status === 401 || r.status === 403) {{ window.location.href = '/admin'; return null; }}
      return r.text();
    }}).then(function(t) {{ if (t !== null) {{ rawLog = t; render(); }} }}).catch(function(){{}});
  }};
  var timer = null;
  window.bbLogsAuto = function() {{
    if (document.getElementById('auto').checked) {{ timer = setInterval(bbLogsLoad, 5000); }}
    else if (timer) {{ clearInterval(timer); timer = null; }}
  }};
  document.getElementById('src').addEventListener('change', bbLogsLoad);
  document.getElementById('lines').addEventListener('change', bbLogsLoad);
  document.getElementById('level').addEventListener('change', render);
  document.getElementById('filter').addEventListener('input', render);
  // Initial download href; the server already painted the first tail.
  document.getElementById('dl').href = '/admin/logs/download?' + qs();
}})();
</script>
</body>
</html>
"#,
        body_html = escape(body),
    )
}
