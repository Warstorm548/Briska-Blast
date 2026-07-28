//! The Chat-Mod stylesheet, appended after the shared `common::CSS`.

/// Page-specific styles, appended after the shared `{CSS}`. A plain const
/// (rather than text inside `format!`) so none of the braces need doubling.
pub(super) const CHATMOD_CSS: &str = "
/* Chat-Mod: full-width three-column moderation layout. The left column width
   is a CSS variable so the drag handle can resize it on desktop. */
.cm-page { width: 100%; }
.cm-card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 24px; }
.cm-layout { display: grid; grid-template-columns: var(--cm-left-w, 280px) minmax(0, 1fr) 260px; gap: 16px; padding-top: 14px; }
.cm-panel { background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 14px; }
.cm-panel-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; padding-bottom: 8px; margin-bottom: 10px; border-bottom: 1px solid #30363d; }
.cm-empty { font-size: 0.82rem; color: #8b949e; text-align: center; padding: 14px 0; }
/* left panel: session cards + desktop resize handle (rides in the grid gap).
   Capped flex column — the title stays put, the card list scrolls inside. */
.cm-left { position: relative; }
.cm-left, .cm-right { display: flex; flex-direction: column; max-height: 80vh; }
.cm-panel-scroll { flex: 1; min-height: 0; overflow-y: auto; }
.cm-resize { position: absolute; top: 0; bottom: 0; right: -12px; width: 8px; border-radius: 4px; cursor: col-resize; touch-action: none; background: none; border: none; padding: 0; }
.cm-resize:hover, .cm-resize:focus-visible, body.cm-resizing .cm-resize { background: #30363d; outline: none; }
body.cm-resizing { cursor: col-resize; user-select: none; }
.cm-session-card { position: relative; display: block; background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 10px 12px; margin-bottom: 10px; text-decoration: none; color: #c9d1d9; }
.cm-session-card:hover { border-color: #8b949e; }
.cm-session-card.cm-session-active { border-color: #e94560; }
.cm-session-code { font-size: 0.72rem; color: #8b949e; margin-bottom: 4px; }
.cm-session-preview { font-size: 0.78rem; color: #8b949e; line-height: 1.5; overflow-wrap: anywhere; }
.cm-dot { position: absolute; top: 9px; right: 9px; width: 10px; height: 10px; border-radius: 50%; background: #f85149; }
/* center column */
.cm-center { min-width: 0; }
.cm-subhead { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding-bottom: 10px; margin-bottom: 14px; border-bottom: 1px solid #30363d; }
.cm-subhead-title { flex: 1; font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; }
.cm-burger { display: none; cursor: pointer; font-size: 1.25rem; line-height: 1; color: #c9d1d9; background: none; border: none; padding: 2px 8px; }
/* landing: flagged messages — an inset panel with cards, mirroring the left panel */
.cm-flag-wrap { background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 16px 14px; min-height: 62vh; max-height: 78vh; display: flex; flex-direction: column; }
.cm-flag-scroll { flex: 1; min-height: 0; overflow-y: auto; }
.cm-flag-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; margin-bottom: 4px; }
.cm-flag-sub { font-size: 0.82rem; color: #8b949e; margin-bottom: 14px; }
.cm-flag-card { display: block; background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 10px 14px; margin-bottom: 12px; text-decoration: none; color: #c9d1d9; }
.cm-flag-card:hover { border-color: #8b949e; }
.cm-flag-code { font-size: 0.85rem; font-weight: 600; margin-bottom: 6px; }
.cm-flag-user { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; font-size: 0.78rem; color: #c9d1d9; margin-bottom: 3px; }
.cm-pid { font-size: 0.72rem; color: #8b949e; }
.cm-flag-body { font-size: 0.84rem; margin-bottom: 8px; overflow-wrap: anywhere; }
.cm-flag-body:last-child { margin-bottom: 0; }
.cm-bodyid { font-size: 0.72rem; color: #8b949e; }
.cm-flag { background: rgba(248, 81, 73, 0.18); color: #f85149; border-radius: 3px; padding: 0 3px; }
/* transcript flags are tappable toggle buttons; selected = solid red */
.cm-flag-btn { font: inherit; border: none; cursor: pointer; }
.cm-flag-btn:hover { background: rgba(248, 81, 73, 0.32); }
.cm-flag-btn:focus-visible { outline: 1px solid #f85149; outline-offset: 1px; }
.cm-flag-sel { background: #f85149; color: #0d1117; font-weight: 600; }
/* session view */
.cm-session-head { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 12px; }
.cm-session-headtext { flex: 1; min-width: 0; }
.cm-session-title { font-size: 0.95rem; font-weight: 600; margin-bottom: 3px; }
.cm-code { color: #e94560; }
.cm-session-sub { font-size: 0.82rem; color: #8b949e; }
.cm-close { color: #8b949e; text-decoration: none; font-size: 1.05rem; line-height: 1; padding: 4px 8px; border-radius: 6px; background: none; border: none; cursor: pointer; font-family: inherit; }
.cm-close:hover { color: #f85149; background: #2d1117; }
.cm-session-body { display: grid; grid-template-columns: minmax(0, 2fr) minmax(240px, 1fr); gap: 16px; }
.cm-chat { display: flex; flex-direction: column; background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 14px; min-height: 56vh; max-height: 76vh; }
.cm-chat-scroll { flex: 1; min-height: 0; overflow-y: auto; }
.cm-msg { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 10px 12px; margin-bottom: 10px; }
.cm-msg-head { display: flex; align-items: center; gap: 10px; font-size: 0.72rem; color: #8b949e; margin-bottom: 6px; }
.cm-msg-user { flex: 1; color: #c9d1d9; }
.cm-msg-body { font-size: 0.85rem; overflow-wrap: anywhere; }
.cm-chatbar { display: flex; gap: 8px; margin-top: 12px; }
/* quick access tools — grouped: player actions / word tools / settings.
   Capped like the chat column; the groups scroll under the pinned title. */
.cm-tools { display: flex; flex-direction: column; max-height: 76vh; }
.cm-tool-group { margin-top: 12px; }
.cm-tool-group + .cm-tool-group { border-top: 1px solid #30363d; margin-top: 18px; padding-top: 14px; }
.cm-tool-group-title { font-size: 0.66rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.1px; color: #8b949e; margin-bottom: 10px; }
/* result of the last tool action; colour set by the reply, not by the markup */
.cm-tool-notice { font-size: 0.72rem; border-radius: 6px; padding: 8px 10px; margin: 0 0 4px; border: 1px solid #30363d; }
.cm-tool-notice-ok { color: #3fb950; border-color: #238636; background: #0f2417; }
.cm-tool-notice-err { color: #f85149; border-color: #8b2c22; background: #2a1513; }
.cm-tool { margin-bottom: 14px; }
.cm-tool:last-child { margin-bottom: 0; }
.cm-tool-btn { width: 100%; }
.cm-check { display: flex; align-items: center; gap: 8px; font-size: 0.84rem; color: #c9d1d9; }
.cm-reason { margin-top: 8px; }
.cm-dur { min-width: 0; }
.cm-approve-all { margin-top: 8px; }
.cm-approve-sel { font-size: 0.74rem; color: #8b949e; margin-top: 6px; }
/* right panel: placeholder secondary nav */
.cm-nav-item { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 9px 8px; font-size: 0.85rem; color: #8b949e; border-bottom: 1px solid #21262d; cursor: not-allowed; }
.cm-soon { font-size: 0.62rem; text-transform: uppercase; letter-spacing: 1px; color: #6e7681; border: 1px solid #30363d; border-radius: 10px; padding: 1px 7px; }
/* click/tap-to-copy ids: subtle affordance, brief green flash on success */
[data-copy] { cursor: copy; }
[data-copy]:hover { color: #c9d1d9; text-decoration: underline dotted; }
[data-copy]:focus-visible { outline: 1px solid #388bfd; outline-offset: 1px; border-radius: 3px; }
[data-copy].cm-copied, [data-copy].cm-copied:hover { color: #3fb950; }
/* mobile: side panels become edge drawers toggled by the corner burgers */
.cm-backdrop { display: none; position: fixed; inset: 0; background: rgba(1, 4, 9, 0.6); z-index: 90; }
@media (max-width: 768px) {
  .cm-card { padding: 20px 16px; }
  .cm-layout { grid-template-columns: 1fr; }
  .cm-burger { display: inline-block; }
  .cm-resize { display: none; }
  .cm-subhead-title { text-align: center; }
  .cm-left, .cm-right { position: fixed; top: 0; height: 100vh; width: 280px; max-width: 85vw; overflow-y: auto; border-radius: 0; background: #161b22; z-index: 100; visibility: hidden; transition: transform .2s ease, visibility 0s linear .2s; }
  .cm-left { left: 0; border-right: 1px solid #30363d; transform: translateX(-100%); max-height: none; }
  .cm-right { right: 0; border-left: 1px solid #30363d; transform: translateX(100%); max-height: none; }
  body.cm-left-open .cm-left, body.cm-right-open .cm-right { transform: translateX(0); visibility: visible; transition: transform .2s ease; }
  body.cm-left-open .cm-backdrop, body.cm-right-open .cm-backdrop { display: block; }
  .cm-session-body { grid-template-columns: 1fr; }
  .cm-chat { min-height: 46vh; }
  /* stacked layout scrolls as one page — no nested scroll inside the tools */
  .cm-tools { max-height: none; }
  .cm-tools .cm-panel-scroll { flex: none; min-height: auto; overflow-y: visible; }
}
/* right nav: the live Chat Audit Logs entry — a link, or the current-page
   marker — overriding the inert placeholders' cursor:not-allowed */
.cm-nav-link { color: #c9d1d9; text-decoration: none; cursor: pointer; }
.cm-nav-link:hover { color: #fff; background: #161b22; }
.cm-nav-link:focus-visible { outline: 1px solid #388bfd; outline-offset: -1px; }
.cm-nav-current { color: #e94560; cursor: default; }
/* audit logs: advanced-filter placeholder bar + wide scrollable ledger table */
.cm-audit-filter { background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 12px 14px; margin-bottom: 14px; }
.cm-audit-filter-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; margin-bottom: 6px; }
.cm-audit-filter-note { font-size: 0.8rem; color: #6e7681; min-height: 34px; }
/* category dropdown: picks which log table + filter renders */
.cm-audit-select { display: flex; align-items: center; gap: 10px; margin-bottom: 14px; }
.cm-audit-select label { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; }
.cm-audit-select select { min-width: 180px; }
/* per-table Advanced Filter: shared spine group beside the table-specific one */
.cm-filter-grid { display: flex; flex-wrap: wrap; gap: 14px; }
.cm-filter-group { flex: 1; min-width: 260px; border: 1px solid #30363d; border-radius: 8px; padding: 6px 14px 14px; margin: 0; display: flex; flex-wrap: wrap; gap: 10px 14px; }
.cm-filter-group legend { font-size: 0.64rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1px; color: #8b949e; padding: 0 6px; }
.cm-filter-group label { display: flex; flex-direction: column; gap: 4px; font-size: 0.7rem; color: #8b949e; }
.cm-filter-group input, .cm-filter-group select { min-width: 130px; }
.cm-audit-list { display: inline-block; background: #21262d; border: 1px solid #30363d; border-radius: 10px; padding: 1px 9px; font-size: 0.74rem; color: #c9d1d9; }
/* System (automated) actor badge — distinct from the red word flags so
   program-initiated rows stand out in any table */
.cm-audit-sys { display: inline-block; background: #10262b; border: 1px solid #1f6f78; border-radius: 10px; padding: 1px 9px; font-size: 0.72rem; font-weight: 600; color: #39c0c8; }
.cm-audit-scroll { border: 1px solid #30363d; border-radius: 8px; overflow: auto; max-height: 68vh; }
.cm-audit-table { width: 100%; border-collapse: collapse; font-size: 0.82rem; white-space: nowrap; }
.cm-audit-table th { position: sticky; top: 0; background: #161b22; text-align: left; font-size: 0.66rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1px; color: #8b949e; padding: 10px 12px; border-bottom: 1px solid #30363d; }
/* sortable column header: label + a flip-arrow (triangle with a line under it)
   that rotates 180° to reverse the row order */
.cm-sort { display: inline-flex; align-items: center; gap: 6px; background: none; border: none; padding: 0; cursor: pointer; font: inherit; color: inherit; text-transform: inherit; letter-spacing: inherit; }
.cm-sort:hover, .cm-sort[data-dir] { color: #c9d1d9; }
.cm-sort:focus-visible { outline: 1px solid #388bfd; outline-offset: 2px; }
.cm-sort-ico { display: inline-block; font-size: 0.85em; line-height: 1; border-bottom: 1px solid currentColor; padding-bottom: 1px; transition: transform .15s ease; }
.cm-sort[data-dir='asc'] .cm-sort-ico { transform: rotate(180deg); }
.cm-audit-table td { padding: 10px 12px; border-bottom: 1px solid #21262d; color: #c9d1d9; vertical-align: top; }
.cm-audit-table tbody tr:last-child td { border-bottom: none; }
.cm-audit-table tbody tr:hover td { background: #10151c; }
.cm-audit-ts { color: #8b949e; }
.cm-audit-words { white-space: normal; }
.cm-audit-words .cm-flag { margin-right: 4px; }
.cm-audit-none { color: #6e7681; }
/* the action-point divider in a full-transcript snapshot (bans). Without it a
   reviewer would read what came after the ban as the evidence that led to it */
.cm-audit-cut { margin: 10px 0; padding: 3px 8px; border-top: 1px dashed #f85149; border-bottom: 1px dashed #f85149; color: #f85149; font-size: 0.7rem; text-transform: uppercase; letter-spacing: 1px; text-align: center; }
/* multi-body cell: a ×N disclosure condensing one player's covered bodies */
.cm-audit-bodies > summary { cursor: pointer; list-style: none; }
.cm-audit-bodies > summary::-webkit-details-marker { display: none; }
.cm-audit-badge { display: inline-block; background: #21262d; border: 1px solid #30363d; border-radius: 10px; padding: 1px 9px; font-size: 0.72rem; color: #c9d1d9; }
.cm-audit-bodies > summary:hover .cm-audit-badge, .cm-audit-bodies[open] > summary .cm-audit-badge { border-color: #8b949e; }
.cm-audit-bodylist { list-style: none; margin: 8px 0 0; padding: 0; }
.cm-audit-bodylist li { padding: 2px 0; }
/* snapshot: mark the message bodies this action actually covered */
.cm-msg-targeted { border-left: 3px solid #d29922; }
.cm-msg-tag { display: inline-block; font-size: 0.6rem; text-transform: uppercase; letter-spacing: 1px; color: #d29922; border: 1px solid #d29922; border-radius: 10px; padding: 0 6px; }
.cm-msg-mod { display: inline-block; font-size: 0.6rem; text-transform: uppercase; letter-spacing: 1px; color: #58a6ff; border: 1px solid #58a6ff; border-radius: 10px; padding: 0 6px; margin-right: 4px; }
.cm-msg-as { color: #8b949e; font-size: 0.72rem; font-style: italic; }
/* a warning sent to one player: purple, the one hue not already spoken for
   (amber = flagged/targeted, blue = moderator chat, red = blacklisted word) */
.cm-msg-warning { border-left: 3px solid #a371f7; }
.cm-msg-warn { display: inline-block; font-size: 0.6rem; text-transform: uppercase; letter-spacing: 1px; color: #a371f7; border: 1px solid #a371f7; border-radius: 10px; padding: 0 6px; margin-right: 4px; }
.cm-msg-sent { color: #a371f7; font-size: 0.72rem; }
/* a chat ban: red, deliberately the same hue as a blacklisted word and the
   client's own ban notice, because it is the panel's one permanent action */
.cm-msg-banned { border-left: 3px solid #f85149; }
.cm-msg-ban { display: inline-block; font-size: 0.6rem; text-transform: uppercase; letter-spacing: 1px; color: #f85149; border: 1px solid #f85149; border-radius: 10px; padding: 0 6px; margin-right: 4px; }
/* undelivered is the state a moderator must not miss: warnings and bans are
   never queued, so this one never reached the player at the time */
.cm-msg-undelivered { color: #f85149; font-size: 0.72rem; font-weight: 700; }
/* deleted bodies stay on the moderation surface — the moderator's copy is the
   record — but read as withdrawn rather than live */
.cm-msg-deleted { background: #0d1117; border-style: dashed; opacity: 0.72; }
.cm-msg-deleted .cm-msg-body { color: #8b949e; text-decoration: line-through; text-decoration-color: #6e7681; }
.cm-msg-removed { font-size: 0.64rem; color: #6e7681; font-style: italic; margin: 0 0 6px; }
/* transcript snapshot overlay: wider/taller than the shared 380px modal card so
   the chat reads cleanly; the shared semi-transparent backdrop keeps the page
   visible behind it */
.cm-audit-modal { max-width: 720px; max-height: 82vh; padding: 0; display: flex; flex-direction: column; }
.cm-audit-modal-head { display: flex; align-items: flex-start; gap: 12px; padding: 20px 22px 14px; border-bottom: 1px solid #30363d; }
.cm-audit-modal-head > div { flex: 1; min-width: 0; }
.cm-audit-modal-head .section-sub { margin-bottom: 0; overflow-wrap: anywhere; }
.cm-audit-modal-scroll { flex: 1; min-height: 0; overflow-y: auto; padding: 16px 22px 20px; }
/* Moderation Lists: sub-tab strip + per-tab tools grid. All cm-lists-* so they
   can't collide with the audit/session styles that share this sheet. */
.cm-lists-tabs { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 16px; }
.cm-lists-tab { font: inherit; font-size: 0.82rem; color: #8b949e; background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 7px 14px; cursor: pointer; }
.cm-lists-tab:hover { border-color: #8b949e; color: #c9d1d9; }
.cm-lists-tab:focus-visible { outline: 1px solid #388bfd; outline-offset: 1px; }
.cm-lists-tab-active, .cm-lists-tab-active:hover { color: #fff; border-color: #e94560; background: #21262d; cursor: default; }
/* the rounded tools panel above each list table (mirrors the mockup's oval) */
.cm-lists-tools { background: #0d1117; border: 1px solid #30363d; border-radius: 12px; padding: 16px; margin-bottom: 18px; display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 16px 20px; align-items: start; }
.cm-lists-tool { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
/* Lock the reason/text inputs to their own height: with align-items:start the
   columns no longer stretch to a shared height, and flex:0 0 auto keeps a growing
   word box from resizing the reason field above or beside it. */
.cm-lists-tool > input { flex: 0 0 auto; }
.cm-lists-tool-title { font-size: 0.72rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1px; color: #8b949e; }
.cm-lists-tool textarea { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; color: #c9d1d9; padding: 8px 10px; font: inherit; font-size: 0.85rem; resize: vertical; min-height: 88px; overflow: hidden; outline: none; width: 100%; }
.cm-lists-tool textarea:focus { border-color: #388bfd; }
.cm-lists-note { font-size: 0.8rem; color: #8b949e; margin-bottom: 14px; }
.cm-lists-search { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; flex-wrap: wrap; }
.cm-lists-search label { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; white-space: nowrap; }
.cm-lists-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; margin-bottom: 10px; }
.cm-lists-hint { font-size: 0.8rem; color: #8b949e; }
/* checkbox + delete cells in the list tables sit centered */
.cm-lists-check { text-align: center; }
";
