//! The admin Stats page — live session/player counts + placeholder metrics.

use super::common::{nav_html, CSS};

pub fn stats_page(session_count: usize, player_count: u64) -> String {
    let nav = nav_html("stats");
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Briska Blast — Admin Stats</title>
  <style>{CSS}</style>
</head>
<body>
<div class="page">
  <div class="card">
    {nav}
    <div style="height:14px"></div>

    <div class="section">
      <p class="section-title">Server Stats</p>
      <div class="stats" style="margin-top:12px">
        <div class="stat-box">
          <div class="stat-num">{session_count}</div>
          <div class="stat-lbl">Active Sessions</div>
        </div>
        <div class="stat-box">
          <div class="stat-num">{player_count}</div>
          <div class="stat-lbl">Total Players</div>
        </div>
      </div>
    </div>

    <div class="section">
      <p class="section-title">Coming Soon</p>
      <p class="section-sub">More server statistics will land here over time.</p>
      <div class="stats" style="margin-top:12px;flex-wrap:wrap">
        <div class="stat-box stat-box-muted" style="min-width:140px">
          <div class="stat-num">&mdash;</div>
          <div class="stat-lbl">Uptime</div>
        </div>
        <div class="stat-box stat-box-muted" style="min-width:140px">
          <div class="stat-num">&mdash;</div>
          <div class="stat-lbl">Peak Players</div>
        </div>
        <div class="stat-box stat-box-muted" style="min-width:140px">
          <div class="stat-num">&mdash;</div>
          <div class="stat-lbl">Sessions Today</div>
        </div>
        <div class="stat-box stat-box-muted" style="min-width:140px">
          <div class="stat-num">&mdash;</div>
          <div class="stat-lbl">Avg Latency</div>
        </div>
      </div>
      <p class="note">No data yet — placeholders for upcoming metrics.</p>
    </div>

  </div>
</div>
</body>
</html>"#
    )
}
