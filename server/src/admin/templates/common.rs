//! Shared building blocks for the admin HTML pages: the HTML-escaper, the
//! single `<style>` sheet reused by every page, and the nav bar (+ idle-timeout
//! modal script) rendered on every authenticated page.

use crate::admin::AdminRole;

/// Minimal HTML-entity escaper for interpolated user/config text.
pub(super) fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(super) const CSS: &str = "
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: system-ui, -apple-system, sans-serif; background: #0d1117; color: #c9d1d9; min-height: 100vh; padding: 24px 16px; }
.page { width: min(92vw, 1280px); margin: 0 auto; }
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
.stat-box-muted { opacity: 0.5; }
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
/* responsive nav: inline tabs on desktop, hamburger + left drawer below 768px */
.nav-links { display: flex; gap: 18px; align-items: center; }
.nav-burger { display: none; cursor: pointer; font-size: 1.4rem; line-height: 1; color: #c9d1d9; background: none; border: none; padding: 2px 8px; }
.nav-backdrop { display: none; position: fixed; inset: 0; background: rgba(1,4,9,0.6); z-index: 90; }
.nav-drawer { position: fixed; top: 0; left: 0; height: 100vh; width: 240px; background: #161b22; border-right: 1px solid #30363d; padding: 20px 16px; z-index: 100; transform: translateX(-100%); visibility: hidden; transition: transform .2s ease, visibility 0s linear .2s; display: flex; flex-direction: column; gap: 4px; }
.nav-drawer .nav-link { display: block; padding: 10px 8px; border-bottom: 1px solid #21262d; border-left: 2px solid transparent; }
.nav-drawer .nav-link-active { border-left-color: #e94560; border-bottom-color: #21262d; }
.drawer-head { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
.drawer-close { cursor: pointer; font-size: 1.1rem; color: #8b949e; background: none; border: none; padding: 2px 6px; line-height: 1; }
.drawer-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; }
.drawer-logout { margin-top: 14px; padding-top: 14px; border-top: 1px solid #30363d; }
nav.drawer-open .nav-drawer { transform: translateX(0); visibility: visible; transition: transform .2s ease; }
nav.drawer-open .nav-backdrop { display: block; }
@media (max-width: 768px) {
  .page { width: 100%; }
  .card { padding: 20px 16px; }
  nav { justify-content: flex-start; gap: 14px; }
  .nav-links, .nav-logout { display: none; }
  .nav-burger { display: inline-block; }
}
";

pub(super) fn nav_html(active: &str, role: AdminRole, username: &str) -> String {
    let dash_active = if active == "dashboard" { " nav-link-active" } else { "" };
    let users_active = if active == "users" { " nav-link-active" } else { "" };
    let stats_active = if active == "stats" { " nav-link-active" } else { "" };
    // Moderators have no Users access, so the link is omitted from both the
    // desktop bar and the drawer (server-side guards enforce this regardless).
    let users_link = if role.at_least(AdminRole::Admin) {
        format!(r#"<a href="/admin/users" class="nav-link{users_active}">Users</a>"#)
    } else {
        String::new()
    };
    let identity_label = format!("{} · {}", escape(username), role.label());
    // Built once and reused in both the desktop top bar (.nav-links) and the
    // mobile drawer (.nav-drawer) so the two link lists can never drift apart.
    let links = format!(
        r#"<a href="/admin/dashboard" class="nav-link{dash_active}">Dashboard</a>
    {users_link}
    <a href="/admin/stats" class="nav-link{stats_active}">Stats</a>"#
    );
    // A real <button> toggles the mobile drawer (with aria-controls /
    // aria-expanded) so keyboard and AT users can open it; a small inline script
    // flips the `drawer-open` class and Escape closes it — mirroring the JS the
    // delete-confirm modal already uses. Burger + drawer are hidden ≥768px via
    // CSS. Links are real <a> navigations; the drawer's duplicate links stay
    // visibility:hidden (out of tab/AT order) until the drawer is opened.
    //
    // The idle-timeout warning modal + timer are appended here (rather than per
    // page) so they ride on every authenticated page but never the login screen,
    // which has no nav. The warn/logout seconds come from the shared server
    // constants so the client countdown can't drift from the Redis session TTL.
    let warn = super::super::ADMIN_IDLE_WARN_SECS;
    let logout = super::super::ADMIN_IDLE_LOGOUT_SECS;
    // Initial countdown shown the instant the warning modal appears, before the
    // 1s JS tick recomputes it — derived from the constants so it can't drift.
    let idle_secs = logout.saturating_sub(warn);
    format!(
        r#"<nav>
  <button type="button" class="nav-burger" aria-label="Open menu" aria-controls="nav-drawer" aria-expanded="false" onclick="bbToggleNav(this)">&#9776;</button>
  <div class="nav-left">
    <span class="brand">Briska Blast Admin</span>
    <span class="nav-links">{links}</span>
  </div>
  <div class="nav-logout">
    <div style="display:flex;align-items:center;gap:10px">
      <span style="color:#6e7681;font-size:0.78rem">{identity_label}</span>
      <form method="POST" action="/admin/logout" style="margin:0">
        <button type="submit" class="btn btn-sm">Logout</button>
      </form>
    </div>
  </div>
  <div class="nav-backdrop" onclick="bbCloseNav()"></div>
  <aside class="nav-drawer" id="nav-drawer">
    <div class="drawer-head">
      <button type="button" class="drawer-close" aria-label="Close menu" onclick="bbCloseNav()">&#10005;</button>
      <span class="drawer-title">Menu</span>
    </div>
    {links}
    <div class="drawer-logout">
      <div style="color:#6e7681;font-size:0.78rem;margin-bottom:8px">{identity_label}</div>
      <form method="POST" action="/admin/logout" style="margin:0">
        <button type="submit" class="btn btn-sm" style="width:100%">Logout</button>
      </form>
    </div>
  </aside>
</nav>
<script>
function bbToggleNav(btn){{var nav=btn.closest('nav');var open=nav.classList.toggle('drawer-open');btn.setAttribute('aria-expanded',open?'true':'false');}}
function bbCloseNav(){{var nav=document.querySelector('nav.drawer-open');if(!nav)return;nav.classList.remove('drawer-open');var b=nav.querySelector('.nav-burger');if(b)b.setAttribute('aria-expanded','false');}}
document.addEventListener('keydown',function(e){{if(e.key==='Escape')bbCloseNav();}});
</script>
<div id="idle-backdrop" class="modal-backdrop">
  <div class="modal-card" role="alertdialog" aria-modal="true" aria-labelledby="idle-title" aria-describedby="idle-desc">
    <p class="section-title" id="idle-title">Still there?</p>
    <p class="section-sub" id="idle-desc">You'll be signed out in <span id="idle-count">{idle_secs}</span> seconds due to inactivity.</p>
    <div class="modal-actions">
      <button type="button" class="btn btn-primary btn-sm" id="idle-stay" onclick="bbStayLoggedIn()">Keep me logged in</button>
    </div>
  </div>
</div>
<script>
(function(){{
  // Idle-session timeout. The server's Redis TTL is the real security boundary;
  // this is the UX layer: warn at {warn}s idle, force-logout at {logout}s. Wall-clock
  // comparison on a 1s tick (not chained timeouts) so a slept/backgrounded tab is
  // correctly treated as idle the moment it wakes.
  //
  // Activity is shared across admin tabs via a localStorage timestamp: effective
  // idle is measured from the most recent activity in ANY tab, so a backgrounded
  // idle tab can't force-logout a tab the user is actively using. ping/lastPing
  // stay per-tab.
  var WARN_MS = {warn} * 1000, LOGOUT_MS = {logout} * 1000, PING_MS = 60000;
  var SHARED_KEY = 'bb_admin_last_activity';
  var last = Date.now(), lastPing = Date.now(), lastShared = 0;
  var warned = false, loggingOut = false;
  var backdrop = document.getElementById('idle-backdrop');
  var countEl = document.getElementById('idle-count');
  function ping(){{
    lastPing = Date.now();
    fetch('/admin/keepalive', {{ method: 'POST' }}).then(function(r){{
      if (r.status === 401) window.location.href = '/admin';
    }}).catch(function(){{}});
  }}
  function hideWarn(){{ if (warned) {{ warned = false; backdrop.style.display = 'none'; }} }}
  function onActivity(){{
    var now = Date.now();
    if (now > last) last = now;
    hideWarn();
    // Broadcast to other tabs (throttled — scroll fires rapidly).
    if (now - lastShared > 1000) {{
      lastShared = now;
      try {{ localStorage.setItem(SHARED_KEY, String(now)); }} catch (e) {{}}
    }}
    // Throttled so active use refreshes the server TTL without spamming it.
    if (now - lastPing > PING_MS) ping();
  }}
  // "Keep me logged in" is just an explicit activity signal (always pings here,
  // since by definition no activity has happened for the last several minutes).
  window.bbStayLoggedIn = onActivity;
  ['mousedown','keydown','scroll','touchstart','click'].forEach(function(ev){{
    document.addEventListener(ev, onActivity, {{ passive: true, capture: true }});
  }});
  // Activity in another tab arrives here in real time; adopt its timestamp.
  window.addEventListener('storage', function(e){{
    if (e.key === SHARED_KEY && e.newValue) {{
      var ts = parseInt(e.newValue, 10) || 0;
      if (ts > last) last = ts;
    }}
  }});
  setInterval(function(){{
    // Effective idle = time since the most recent activity in ANY admin tab.
    var shared = 0;
    try {{ shared = parseInt(localStorage.getItem(SHARED_KEY), 10) || 0; }} catch (e) {{}}
    var idle = Date.now() - Math.max(last, shared);
    if (idle >= LOGOUT_MS) {{
      if (loggingOut) return;            // navigation in flight — don't double-fire
      loggingOut = true;
      fetch('/admin/logout', {{ method: 'POST' }}).catch(function(){{}}).finally(function(){{
        window.location.href = '/admin';
      }});
      return;
    }}
    if (idle >= WARN_MS) {{
      if (!warned) {{
        warned = true;
        backdrop.style.display = 'flex';
        if (document.getElementById('idle-stay')) document.getElementById('idle-stay').focus();
      }}
      countEl.textContent = Math.ceil((LOGOUT_MS - idle) / 1000);
    }} else {{
      hideWarn();                        // another tab's activity un-idled us
    }}
  }}, 1000);
}})();
</script>"#
    )
}
