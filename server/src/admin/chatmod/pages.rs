//! The four server-rendered Chat-Mod views: the landing page, the entered
//! session, and the two Chat Nav sub-pages (Chat Audit Logs, Moderation Lists).
//! Each resolves what it needs through [`super::super::chatmod_data`] and hands
//! it to the matching template.

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use super::super::{chatmod_data, require_session, templates};
use crate::chat::transcript;
use crate::state::AppState;


/// GET /admin/chatmod — the Chat-Mod landing page (flagged-message overview).
pub async fn chatmod_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Any authenticated role (Moderator and up) may work the moderation panel.
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };

    // Reconcile transcripts whose session died by passive TTL expiry, which runs
    // no teardown code at all. This page already enumerates live sessions, so the
    // sweep rides along on a pass we were making anyway.
    transcript::sweep_orphans(&state).await;

    let (sessions, flagged) = chatmod_data::landing_view(&state).await;
    Html(templates::chatmod_landing_page(
        &sessions,
        &flagged,
        session.role,
        &session.username,
    ))
    .into_response()
}

/// GET /admin/chatmod/session/:code — the entered-session view. Unknown codes
/// bounce back to the landing page rather than 404ing.
pub async fn chatmod_session_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let Some(canon) = chatmod_data::resolve_live_session(&state, &code).await else {
        return Redirect::to("/admin/chatmod").into_response();
    };

    let (sessions, transcript) = chatmod_data::session_view(&state, &canon).await;
    Html(templates::chatmod_session_page(
        &canon,
        &transcript,
        &sessions,
        session.role,
        &session.username,
    ))
    .into_response()
}

/// Shared query for the Chat Nav sub-pages. `from` names the session the
/// moderator was viewing when they opened a sub-page (Moderation Lists / Chat
/// Audit Logs), so the X close can return there instead of the landing page.
#[derive(serde::Deserialize)]
pub struct FromQuery {
    from: Option<String>,
    /// Success / failure notice set by an action's redirect, rendered as the
    /// same banner the Users and Dashboard tabs use.
    #[serde(default)]
    ok: Option<String>,
    #[serde(default)]
    err: Option<String>,
}

impl FromQuery {
    /// `(is_success, message)`, preferring an error when both somehow appear.
    fn notice(&self) -> Option<(bool, &str)> {
        if let Some(e) = self.err.as_deref() {
            return Some((false, e));
        }
        self.ok.as_deref().map(|m| (true, m))
    }
}

/// GET /admin/chatmod/audit — the Chat Audit Logs page. The X close returns to
/// the session named by `?from=` (when it resolves) or to the landing page.
pub async fn chatmod_audit_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FromQuery>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let from = match query.from.as_deref() {
        Some(code) => chatmod_data::resolve_live_session(&state, code).await,
        None => None,
    };

    Html(templates::chatmod_audit_page(
        &chatmod_data::audit_log(&state).await,
        &chatmod_data::live_sessions(&state).await,
        from.as_deref(),
        session.role,
        &session.username,
    ))
    .into_response()
}

/// GET /admin/chatmod/lists — the Moderation Lists page. The X close returns to
/// the session named by `?from=` (when it resolves) or to the landing page,
/// mirroring the Chat Audit Logs page's remember-where-you-were behavior.
pub async fn chatmod_lists_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FromQuery>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let from = match query.from.as_deref() {
        Some(code) => chatmod_data::resolve_live_session(&state, code).await,
        None => None,
    };

    Html(templates::chatmod_lists_page(
        &chatmod_data::moderation_lists(&state).await,
        &chatmod_data::live_sessions(&state).await,
        from.as_deref(),
        query.notice(),
        session.role,
        &session.username,
    ))
    .into_response()
}
