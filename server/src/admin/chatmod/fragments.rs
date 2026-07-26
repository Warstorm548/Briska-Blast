//! The live-refresh endpoints the Chat-Mod pages poll.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
    Json,
};

use super::super::{chatmod_data, require_session, templates};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Live refresh
//
// Both endpoints return HTML fragments the page swaps into place, rather than
// JSON the browser re-renders — the server already owns this markup and its
// escaping, and duplicating either in JavaScript is how the two drift apart.
// The polling contract (redirect / 401 / 403 → back to the login page) matches
// the Logs tab.
// ---------------------------------------------------------------------------

/// GET /admin/chatmod/data — the landing page's two panels.
pub async fn chatmod_data_fragment(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    let (sessions, flagged) = chatmod_data::landing_view(&state).await;
    Json(serde_json::json!({
        "sessions": templates::chatmod_sessions_fragment(&sessions, None),
        "flagged": templates::chatmod_flagged_fragment(&flagged),
    }))
    .into_response()
}

/// GET /admin/chatmod/session/:code/data — the entered session's transcript,
/// plus the left panel so other sessions' previews and red dots stay current
/// while a moderator is inside one.
pub async fn chatmod_session_data(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    let Some(canon) = chatmod_data::resolve_live_session(&state, &code).await else {
        // The session ended under the moderator. Send them back to the landing
        // page rather than leaving them polling a transcript that is now gone.
        return Redirect::to("/admin/chatmod").into_response();
    };
    let (sessions, transcript) = chatmod_data::session_view(&state, &canon).await;
    Json(serde_json::json!({
        "sessions": templates::chatmod_sessions_fragment(&sessions, Some(&canon)),
        "transcript": templates::chatmod_transcript_fragment(&transcript),
    }))
    .into_response()
}
