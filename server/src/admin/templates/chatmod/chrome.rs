//! Fixed furniture shared by every Chat-Mod view: the center sub-header, the
//! right-hand Chat Nav, and the session view's Quick Access Tools panel.

/// The center sub-header shared by both views: title bar with a burger at each
/// corner. The burgers are hidden ≥768px; below that they slide the left
/// (sessions) / right (chat nav) panels in as drawers.
pub(super) const SUBHEAD_HTML: &str = r#"<div class="cm-subhead">
  <button type="button" class="cm-burger" aria-label="Open sessions panel" aria-controls="cm-left" aria-expanded="false" onclick="bbCmToggle(this,'left')">&#9776;</button>
  <span class="cm-subhead-title">Chat Moderation Area</span>
  <button type="button" class="cm-burger" aria-label="Open chat nav panel" aria-controls="cm-right" aria-expanded="false" onclick="bbCmToggle(this,'right')">&#9776;</button>
</div>"#;

/// Which Chat Nav sub-page is currently rendered. Drives the live-link vs
/// current-page-marker choice for the two wired entries (Moderation Lists, Chat
/// Audit Logs); `None` for the landing/session views, where neither is open.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum ChatNavPage {
    None,
    Lists,
    Audit,
}

/// The right-hand "Chat Nav" secondary navigation. Two entries stay inert visual
/// placeholders until their sub-pages are designed (Settings, Mod User Settings);
/// "Moderation Lists" and "Chat Audit Logs" are live — each a link to its
/// `?from=`-carrying href, or the active current-page marker (no href) when it is
/// the page being rendered. The old standalone "Suspensions" entry now lives as
/// the *Active Suspensions* sub-tab inside Moderation Lists.
pub(super) fn chat_nav_html(lists_href: &str, audit_href: &str, current: ChatNavPage) -> String {
    // One live entry: the current-page marker when it's the open page, else a
    // link that carries the session context forward.
    let entry = |label: &str, href: &str, is_current: bool| {
        if is_current {
            format!(r#"<span class="cm-nav-item cm-nav-current" aria-current="page">{label}</span>"#)
        } else {
            format!(r#"<a class="cm-nav-item cm-nav-link" href="{href}">{label}</a>"#)
        }
    };
    let lists = entry("Moderation Lists", lists_href, current == ChatNavPage::Lists);
    let audit = entry("Chat Audit Logs", audit_href, current == ChatNavPage::Audit);
    format!(
        r#"<span class="cm-nav-item">Settings<span class="cm-soon">soon</span></span>
        {lists}
        {audit}
        <span class="cm-nav-item">Mod User Settings<span class="cm-soon">soon</span></span>"#
    )
}

/// The session view's "Quick Access Tools" panel.
///
/// Warn, Warn + Delete, Ban and Blacklist Words post through `fetch` (see
/// `script.rs`) rather than as forms, because the page is polling a live
/// transcript — a redirect would cost the moderator their place in the
/// conversation. Suspend remains an inert placeholder.
///
/// Stays a `const` with no interpolation: the session code the handlers need is
/// already page-wide on `<body data-cm-code>`, which is how `bbCmSay` reaches it.
pub(super) const TOOLS_HTML: &str = r#"<div class="cm-panel cm-tools">
      <p class="cm-panel-title">Quick Access Tools</p>
      <div class="cm-panel-scroll">
      <p class="cm-tool-notice" id="cm-tool-notice" role="status" hidden></p>
      <div class="cm-tool-group">
        <p class="cm-tool-group-title">Player Actions</p>
        <div class="cm-tool">
          <label for="cm-target">Target Player IDs</label>
          <input type="text" id="cm-target" placeholder="Player IDs &mdash; separate with ;">
          <p class="cm-approve-sel">Used by Warn, Suspend &amp; Ban. Ticking message checkboxes adds each body's sender.</p>
        </div>
        <div class="cm-tool">
          <button type="button" class="btn btn-sm cm-tool-btn" onclick="bbCmWarn(1)">Warn + Delete Chat Body</button>
          <input type="text" class="cm-reason" id="cm-warn-del-reason" placeholder="Reason (logged &amp; sent to player)">
          <p class="cm-approve-sel">Deletes the ticked messages for every player still connected.</p>
        </div>
        <div class="cm-tool">
          <button type="button" class="btn btn-sm cm-tool-btn" onclick="bbCmWarn(0)">Warn Only</button>
          <input type="text" class="cm-reason" id="cm-warn-reason" placeholder="Reason (logged &amp; sent to player)">
        </div>
        <div class="cm-tool">
          <label>Suspend User</label>
          <div class="row">
            <input type="text" class="cm-dur" inputmode="numeric" placeholder="Days" aria-label="Days">
            <input type="text" class="cm-dur" inputmode="numeric" placeholder="Hours" aria-label="Hours">
            <input type="text" class="cm-dur" inputmode="numeric" placeholder="Mins" aria-label="Minutes">
            <button type="button" class="btn btn-sm">Suspend</button>
          </div>
          <input type="text" class="cm-reason" placeholder="Reason (logged &amp; sent to player)">
          <p class="cm-approve-sel">At least one duration field is required.</p>
        </div>
        <div class="cm-tool">
          <label>Ban User (Chat)</label>
          <div class="row">
            <input type="text" id="cm-ban-reason" placeholder="Reason (logged &amp; sent to player)">
            <button type="button" class="btn btn-danger btn-sm" onclick="bbCmBanAsk()">Ban</button>
          </div>
        </div>
      </div>
      <div class="cm-tool-group">
        <p class="cm-tool-group-title">Word Tools</p>
        <div class="cm-tool">
          <label for="cm-blacklist">Blacklist Words</label>
          <div class="row">
            <input type="text" id="cm-blacklist" placeholder="Word or words &mdash; separate with ;">
            <button type="button" class="btn btn-sm" onclick="bbCmBlacklist()">Add</button>
          </div>
          <input type="text" class="cm-reason" id="cm-bl-reason" placeholder="Reason (logged)">
          <p class="cm-approve-sel">Censors future messages only &mdash; what players already saw is unchanged.</p>
        </div>
        <div class="cm-tool">
          <button type="button" class="btn btn-sm cm-tool-btn">Approve Word</button>
          <p class="cm-approve-sel">Restores the word in this chat &mdash; blacklist unchanged.</p>
          <label class="cm-check cm-approve-all"><input type="checkbox" id="cm-approve-all"> Select all matching words</label>
          <p class="cm-approve-sel" id="cm-approve-sel">Tap a red word in the chat to select it.</p>
        </div>
      </div>
      <div class="cm-tool-group">
        <p class="cm-tool-group-title">Moderator Chat Settings</p>
        <label class="cm-check"><input type="checkbox" id="cm-show-name"> Appear As Your Display Name</label>
      </div>
      <p class="note">Suspend and Approve Word are not wired up yet.</p>
      </div>
    </div>
    <div id="cm-ban-modal" class="modal-backdrop" onclick="if(event.target===this)bbCmBanClose()">
      <div class="modal-card" role="alertdialog" aria-modal="true" aria-labelledby="cm-ban-title" aria-describedby="cm-ban-desc">
        <p class="section-title" id="cm-ban-title">Confirm Chat Ban</p>
        <p class="section-sub" id="cm-ban-desc">Permanently remove chat privileges for <span id="cm-ban-who" class="mono"></span>?<br>Reason: <span id="cm-ban-why"></span><br>The player keeps playing; reversible only via Moderation Lists.</p>
        <div class="modal-actions">
          <button type="button" class="btn btn-sm" id="cm-ban-cancel" onclick="bbCmBanClose()">Cancel</button>
          <button type="button" class="btn btn-danger btn-sm" onclick="bbCmBanConfirm()">Confirm Chat Ban</button>
        </div>
      </div>
    </div>"#;
