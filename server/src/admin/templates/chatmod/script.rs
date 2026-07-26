//! The Chat-Mod page script, shared by every view.
//!
//! A plain const rather than text inside the shell's `format!` — exactly the
//! reason `style::CHATMOD_CSS` is one — so none of the JavaScript's braces need
//! doubling. The block is fully static; nothing here is interpolated.

pub(super) const CHATMOD_JS: &str = r#"function bbCmToggle(btn,side){var cls=side==='left'?'cm-left-open':'cm-right-open';document.body.classList.remove(side==='left'?'cm-right-open':'cm-left-open');var open=document.body.classList.toggle(cls);btn.setAttribute('aria-expanded',open?'true':'false');}
function bbCmClose(){document.body.classList.remove('cm-left-open','cm-right-open');document.querySelectorAll('.cm-burger').forEach(function(b){b.setAttribute('aria-expanded','false');});}
document.addEventListener('keydown',function(e){if(e.key==='Escape')bbCmClose();});
// Chat opens scrolled to the newest message, chat-app style; the flex layout
// keeps the moderator chat bar pinned to the container's bottom edge.
(function(){var sc=document.querySelector('.cm-chat-scroll');if(sc)sc.scrollTop=sc.scrollHeight;})();
// Word selection for Approve: each red word in the transcript toggles
// individually; the "Select all matching words" checkbox widens a tap to
// every occurrence of that word. Selection state lives on the buttons
// (cm-flag-sel class), the hint line aggregates it per word.
window.bbCmFlagToggle=function(btn){
  var on=!btn.classList.contains('cm-flag-sel');
  var all=document.getElementById('cm-approve-all');
  var targets=(all&&all.checked)?document.querySelectorAll('.cm-flag-btn[data-word="'+CSS.escape(btn.getAttribute('data-word'))+'"]'):[btn];
  targets.forEach(function(b){b.classList.toggle('cm-flag-sel',on);b.setAttribute('aria-pressed',on?'true':'false');});
  var el=document.getElementById('cm-approve-sel');
  if(!el)return;
  var counts={};
  document.querySelectorAll('.cm-flag-btn.cm-flag-sel').forEach(function(b){var w=b.getAttribute('data-word');counts[w]=(counts[w]||0)+1;});
  var parts=Object.keys(counts).map(function(w){return counts[w]>1?w+' ×'+counts[w]:w;});
  el.textContent=parts.length?('Selected: '+parts.join(', ')):'Tap a red word in the chat to select it.';
};
// Click/tap-to-copy for player ids + body ids: one delegated listener over
// [data-copy] elements. preventDefault stops the landing cards' link
// navigation when the tap lands on an id. Clipboard API first (secure
// contexts), hidden-textarea execCommand fallback otherwise.
function bbCmDoCopy(el){
  var v=el.getAttribute('data-copy');
  function done(){el.classList.add('cm-copied');setTimeout(function(){el.classList.remove('cm-copied');},900);}
  function fallback(){
    var t=document.createElement('textarea');t.value=v;t.style.position='fixed';t.style.opacity='0';
    document.body.appendChild(t);t.select();
    try{document.execCommand('copy');}catch(e){}
    document.body.removeChild(t);done();
  }
  if(navigator.clipboard&&navigator.clipboard.writeText){navigator.clipboard.writeText(v).then(done).catch(fallback);}
  else{fallback();}
}
document.addEventListener('click',function(e){
  var el=e.target.closest('[data-copy]');
  if(!el)return;
  e.preventDefault();
  bbCmDoCopy(el);
});
document.addEventListener('keydown',function(e){
  if(e.key!=='Enter'&&e.key!==' ')return;
  var el=e.target.closest&&e.target.closest('[data-copy]');
  if(!el)return;
  e.preventDefault();
  bbCmDoCopy(el);
});
// Message checkboxes feed the Target Player IDs field, a ;-separated list
// (same separator convention as Blacklist Words). Ticking a body appends its
// sender once; unticking removes the id only when no other ticked body still
// carries it. Manually typed ids survive — the list is edited per-id, never
// overwritten wholesale. Only elements carrying data-pid reach this handler.
document.addEventListener('change',function(e){
  var cb=e.target;
  if(!cb.getAttribute||cb.getAttribute('data-pid')===null)return;
  var t=document.getElementById('cm-target');
  if(!t)return;
  var pid=cb.getAttribute('data-pid');
  var list=t.value.split(';').map(function(s){return s.trim();}).filter(function(s){return s;});
  if(cb.checked){
    if(list.indexOf(pid)===-1)list.push(pid);
  }else if(!document.querySelector('input[data-pid="'+CSS.escape(pid)+'"]:checked')){
    list=list.filter(function(s){return s!==pid;});
  }
  t.value=list.join('; ');
});
// Ban requires an explicit confirmation: the modal echoes the target ids and
// reason for review. Cancel / backdrop / Escape back out — a cancelled ban is
// never sent and writes no audit record. Confirm just closes for now; the
// wiring phase swaps it for the real send.
window.bbCmBanAsk=function(){
  var m=document.getElementById('cm-ban-modal');if(!m)return;
  var t=document.getElementById('cm-target'),r=document.getElementById('cm-ban-reason');
  document.getElementById('cm-ban-who').textContent=(t&&t.value.trim())?t.value.trim():'(no target set)';
  document.getElementById('cm-ban-why').textContent=(r&&r.value.trim())?r.value.trim():'(none)';
  m.style.display='flex';
  var c=document.getElementById('cm-ban-cancel');if(c)c.focus();
};
window.bbCmBanClose=function(){var m=document.getElementById('cm-ban-modal');if(m)m.style.display='none';};
document.addEventListener('keydown',function(e){if(e.key==='Escape')bbCmBanClose();});
// Audit-log transcript snapshots: each Chat Audit Logs row's Transcript button
// opens that row's own hidden overlay (index-aligned with the table). Close via
// the overlay X, Escape, or a backdrop click. Preview only — the wiring phase
// fetches the real snapshot on demand instead of pre-rendering one per row.
window.bbCmAuditOpen=function(i){var m=document.getElementById('cm-audit-back-'+i);if(m)m.style.display='flex';};
window.bbCmAuditCloseAll=function(){document.querySelectorAll('.cm-audit-back').forEach(function(m){m.style.display='none';});};
document.addEventListener('keydown',function(e){if(e.key==='Escape')bbCmAuditCloseAll();});
// Chat Audit Logs category dropdown: show the picked category's view, hide the
// rest. Each view carries its own table + Advanced Filter, so switching swaps
// both at once.
window.bbCmAuditCat=function(sel){document.querySelectorAll('.cm-audit-view').forEach(function(v){v.hidden=(v.getAttribute('data-cat')!==sel.value);});};
// Sortable audit columns (Timestamp, Player ID): reorder the table's rows by the
// clicked column and flip the arrow. First click sorts ascending (reversing the
// default newest-first / server order); clicking again toggles. Only one column
// sorts a table at a time. Timestamps (ISO-ish) and zero-padded ids compare
// correctly with a numeric-aware string compare.
window.bbCmSort=function(btn){
  var th=btn.closest('th'), table=btn.closest('table');
  if(!th||!table||!table.tBodies[0])return;
  var col=th.cellIndex, tbody=table.tBodies[0];
  var dir=btn.getAttribute('data-dir')==='asc'?'desc':'asc';
  table.querySelectorAll('.cm-sort').forEach(function(b){if(b!==btn){b.removeAttribute('data-dir');var ot=b.closest('th');if(ot)ot.setAttribute('aria-sort','none');}});
  btn.setAttribute('data-dir',dir);
  th.setAttribute('aria-sort',dir==='asc'?'ascending':'descending');
  var rows=Array.prototype.slice.call(tbody.rows);
  rows.sort(function(a,b){
    var x=(a.cells[col]?a.cells[col].textContent:'').trim();
    var y=(b.cells[col]?b.cells[col].textContent:'').trim();
    var r=x.localeCompare(y,undefined,{numeric:true});
    return dir==='asc'?r:-r;
  });
  rows.forEach(function(r){tbody.appendChild(r);});
};
// Desktop-only resize of the sessions panel: drag writes the grid's CSS
// variable, localStorage persists it across the landing/session page loads.
(function(){
  var KEY='bb_cm_left_w', MIN=200, MAX=520, root=document.documentElement;
  var saved=parseInt(localStorage.getItem(KEY)||'',10);
  if(saved)root.style.setProperty('--cm-left-w',Math.min(MAX,Math.max(MIN,saved))+'px');
  var h=document.getElementById('cm-resize'), panel=document.getElementById('cm-left');
  if(!h||!panel)return;
  function apply(w){w=Math.min(MAX,Math.max(MIN,Math.round(w)));root.style.setProperty('--cm-left-w',w+'px');try{localStorage.setItem(KEY,String(w));}catch(e){}}
  h.addEventListener('pointerdown',function(e){
    e.preventDefault();
    var startX=e.clientX, startW=panel.getBoundingClientRect().width;
    h.setPointerCapture(e.pointerId);
    document.body.classList.add('cm-resizing');
    function move(ev){apply(startW+(ev.clientX-startX));}
    function up(ev){h.releasePointerCapture(ev.pointerId);h.removeEventListener('pointermove',move);h.removeEventListener('pointerup',up);h.removeEventListener('pointercancel',up);document.body.classList.remove('cm-resizing');}
    h.addEventListener('pointermove',move);
    h.addEventListener('pointerup',up);
    h.addEventListener('pointercancel',up);
  });
  h.addEventListener('dblclick',function(){root.style.removeProperty('--cm-left-w');try{localStorage.removeItem(KEY);}catch(e){}});
  h.addEventListener('keydown',function(e){
    if(e.key!=='ArrowLeft'&&e.key!=='ArrowRight')return;
    e.preventDefault();
    apply(panel.getBoundingClientRect().width+(e.key==='ArrowRight'?16:-16));
  });
})();
// Moderation Lists sub-tabs: a horizontal tab strip toggling the four list
// panels. Client-side only — the same show/hide idea as the audit dropdown.
window.bbCmListsTab=function(btn){
  var tab=btn.getAttribute('data-tab');
  document.querySelectorAll('.cm-lists-tab').forEach(function(b){
    var on=b.getAttribute('data-tab')===tab;
    b.classList.toggle('cm-lists-tab-active',on);
    b.setAttribute('aria-selected',on?'true':'false');
  });
  document.querySelectorAll('.cm-lists-panel').forEach(function(p){
    p.hidden=p.getAttribute('data-tab')!==tab;
  });
};
// Blacklist word boxes grow to fit their contents (the mockup's "flex larger in
// height when many words are added"); manual resize still works via CSS.
(function(){
  function grow(t){t.style.height='auto';t.style.height=t.scrollHeight+'px';}
  document.querySelectorAll('.cm-lists-tool textarea').forEach(function(t){
    grow(t);t.addEventListener('input',function(){grow(t);});
  });
})();
// Moderation Lists confirm dialogs (delete word / ban / un-ban): open by id,
// close on Cancel / backdrop / Escape. Preview only — Confirm just closes; the
// wiring phase swaps it for the real action.
window.bbCmListsAsk=function(id){var m=document.getElementById(id);if(m)m.style.display='flex';};
window.bbCmListsCloseAll=function(){document.querySelectorAll('.cm-lists-modal').forEach(function(m){m.style.display='none';});};
document.addEventListener('keydown',function(e){if(e.key==='Escape')bbCmListsCloseAll();});
// Deleting a blacklisted word: the trash icon names the word in the confirm
// dialog, and only Confirm submits. Cancel, Escape or a backdrop click send
// nothing and write no audit record.
//
// The word arrives via a data attribute rather than an inline call argument:
// `escape` does not escape single quotes, so a word like "don't" would otherwise
// terminate the JS string literal.
window.bbCmListsDelete=function(word){
  var w=document.getElementById('cm-lists-del-word');
  var label=document.getElementById('cm-lists-del-who');
  var why=document.getElementById('cm-lists-del-why');
  if(w)w.value=word;
  if(label)label.textContent=word;
  if(why)why.value='';
  bbCmListsAsk('cm-lists-del-modal');
};
window.bbCmListsDeleteConfirm=function(){
  var f=document.getElementById('cm-lists-del-form');
  var why=document.getElementById('cm-lists-del-why');
  var r=document.getElementById('cm-lists-del-reason');
  if(!f)return;
  if(r&&why)r.value=why.value;
  f.submit();
};
// Live refresh. Same contract as the Logs tab: a redirected response or a
// 401/403 means the admin session lapsed, so bounce to the login page rather
// than silently polling a dead session. Inside a session the transcript refreshes
// every 2s; the landing panels are calmer at 5s.
//
// Note this deliberately does NOT touch /admin/keepalive — the idle logout is
// activity-driven, and a background poll must not masquerade as activity.
window.bbCmPollNow=function(){};
(function(){
  var code=document.body.getAttribute('data-cm-code');
  var url=code?('/admin/chatmod/session/'+encodeURIComponent(code)+'/data'):'/admin/chatmod/data';
  var busy=false;
  function apply(id,html){if(html===undefined||html===null)return;var el=document.getElementById(id);if(el)el.innerHTML=html;}
  function load(){
    // One request in flight at a time. On a slow link a 2s interval would
    // otherwise stack requests faster than they complete, and out-of-order
    // replies could render an older transcript over a newer one.
    if(busy)return;
    busy=true;
    fetch(url,{headers:{'Accept':'application/json'}}).then(function(r){
      if(r.redirected){window.location.href=r.url;return null;}
      if(r.status===401||r.status===403){window.location.href='/admin';return null;}
      return r.ok?r.json():null;
    }).then(function(d){
      busy=false;
      if(!d)return;
      apply('cm-sessions',d.sessions);
      apply('cm-flagged',d.flagged);
      var chat=document.getElementById('cm-chat');
      if(chat&&d.transcript!==undefined&&d.transcript!==null){
        // Only follow the tail if the moderator was already reading it. Yanking
        // them to the bottom while they scroll back through history is the
        // single most annoying thing a live chat view can do.
        var atBottom=(chat.scrollHeight-chat.scrollTop-chat.clientHeight)<40;
        chat.innerHTML=d.transcript;
        if(atBottom)chat.scrollTop=chat.scrollHeight;
      }
    }).catch(function(){busy=false;});
  }
  window.bbCmPollNow=load;
  // A hidden tab has nobody reading it; polling it just burns the moderator's
  // battery and the server's Redis. Catch up as soon as it comes back.
  setInterval(function(){if(!document.hidden)load();},code?2000:5000);
  document.addEventListener('visibilitychange',function(){if(!document.hidden)load();});
})();
// Moderator chat. Enter and the Send button run the same path, so desktop and
// the mobile drawer layout behave identically. Posting via fetch (rather than a
// form navigation) keeps scroll position and the poll timer alive.
//
// The checkbox is "Appear As Your Display Name", so UNCHECKED is anonymous —
// and that is the default. The choice is remembered per browser, because a
// moderator who deliberately posts anonymously should not have a fresh page
// load quietly reveal their name on the next message.
(function(){
  var KEY='bb_cm_show_name', c=document.getElementById('cm-show-name');
  if(!c)return;
  try{c.checked=localStorage.getItem(KEY)==='1';}catch(e){}
  c.addEventListener('change',function(){try{localStorage.setItem(KEY,c.checked?'1':'0');}catch(e){}});
})();
window.bbCmSay=function(){
  var input=document.getElementById('cm-chatbar-input');
  var code=document.body.getAttribute('data-cm-code');
  if(!input||!code)return;
  var text=input.value.trim();
  if(!text)return;
  var show=document.getElementById('cm-show-name');
  var body=new URLSearchParams();
  body.set('text',text);
  body.set('show_name',(show&&show.checked)?'1':'0');
  // Clear optimistically: the line is echoed back by the next poll, the same
  // way the game client renders only what the server broadcast.
  input.value='';
  // Restore what they typed if the send didn't land. Optimistic clearing is
  // right when it works, but silently eating a moderator's message on a dropped
  // connection is not — they would have no idea it never arrived.
  function restore(){ if(!input.value)input.value=text; }
  fetch('/admin/chatmod/session/'+encodeURIComponent(code)+'/say',{
    method:'POST',
    headers:{'Content-Type':'application/x-www-form-urlencoded'},
    body:body.toString()
  }).then(function(r){
    if(r.redirected){window.location.href=r.url;return;}
    if(r.status===401||r.status===403){window.location.href='/admin';return;}
    // 404 means the session ended under them; anything else non-OK is a server
    // problem. Either way the line was not delivered, so give it back.
    if(!r.ok){restore();return;}
    bbCmPollNow();
  }).catch(restore);
};"#;
