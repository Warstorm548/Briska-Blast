//! Moderator chat — speaking into a live session from the panel.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};

use super::super::{chatmod_data, require_session};
// `MAX_CHAT_LEN` is shared with the player path, not redefined: a moderator's
// message reaches the same channel, and two copies of the bound would let one
// drift and quietly disagree about what a message may contain.
use crate::chat::MAX_CHAT_LEN;
use crate::state::AppState;

/// The display name a moderator posts under when the "Appear As Your Display
/// Name" toggle is off. Generic on purpose: players learn that a moderator is
/// present without learning which one.
const ANONYMOUS_MODERATOR_NAME: &str = "Mod";

// ---------------------------------------------------------------------------
// Moderator chat
// ---------------------------------------------------------------------------

/// Form body for a moderator message. `show_name` mirrors the "Appear As Your
/// Display Name" checkbox — absent or `0` means post as the generic `Mod`.
#[derive(serde::Deserialize)]
pub struct SayForm {
    text: String,
    #[serde(default)]
    show_name: String,
}

/// POST /admin/chatmod/session/:code/say — speak into a live session's chat.
///
/// The broadcast may be anonymous; **the record never is**. The transcript
/// always stores the acting moderator's display name and Pocket ID subject
/// alongside an `anonymous` flag, so an anonymous intervention is still
/// attributable. A moderator message also retains the session's transcript
/// permanently — a deliberate intervention in a player-facing channel stays on
/// the record.
pub async fn chatmod_say(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Form(form): Form<SayForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let Some(canon) = chatmod_data::resolve_live_session(&state, &code).await else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // Same trim/bound as a player message (`signaling::ws::frame`), so a
    // moderator cannot post an empty line or an oversized one either.
    let text = form.text.trim();
    if text.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let text: String = text.chars().take(MAX_CHAT_LEN).collect();

    let show_name = form.show_name == "1";
    let display = if show_name {
        session.username.clone()
    } else {
        ANONYMOUS_MODERATOR_NAME.to_string()
    };

    crate::chat::speak_as_moderator(
        &state,
        &canon,
        crate::chat::ModeratorMessage {
            display_name: &display,
            moderator: &session.username,
            moderator_sub: &session.sub,
            anonymous: !show_name,
            text: &text,
        },
    )
    .await;

    StatusCode::NO_CONTENT.into_response()
}
