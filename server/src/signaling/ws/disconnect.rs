//! Disconnect cleanup orchestration: the grace windows a dropped mid-game player
//! gets before their slot is freed (or, for a host, the next player is
//! promoted). These functions arm timers and announce the "reconnecting…"
//! overlays; the atomic Redis mutations they trigger live in
//! [`super::session_ops`].

use deadpool_redis::redis::AsyncCommands;
use std::time::Duration;

use super::session_ops::{promote_demote_or_end_active, remove_joiner_on_leave};
use crate::{
    api::Session,
    signaling::{protocol::ServerMsg, GraceKind},
    state::AppState,
};
use shared::types::session::SessionStatus;

/// How long a mid-game host has to re-Identify after their WebSocket drops
/// before the server promotes the next player in join order (or ends the
/// session). Also the duration peers show a "reconnecting…" overlay (broadcast
/// in `HostReconnecting` / `PeerReconnecting`). Const for now; promoting it to
/// runtime config is a later refinement.
const PROMOTION_GRACE: Duration = Duration::from_secs(30);

/// How long ANY dropped mid-game player's session slot is held so they can
/// rejoin by re-entering the code, before it is freed permanently. Longer than
/// `PROMOTION_GRACE` so a full process relaunch + manual code entry can make it
/// back. A promoted-away ex-host is demoted into `joiners` (kept, not removed)
/// and keeps the remainder of this window, rejoining as a non-host.
const RECONNECT_GRACE: Duration = Duration::from_secs(120);

/// Broadcast a `PeerLeft` with the right reason for a deliberate vs transient
/// departure (the common shape used across the cleanup branches).
pub(super) async fn broadcast_peer_left(state: &AppState, code: &str, player_id: &str, explicit_leave: bool) {
    state
        .signal_hub
        .broadcast(
            code,
            ServerMsg::PeerLeft {
                player_id: player_id.to_string(),
                reason: if explicit_leave { "leave" } else { "disconnect" },
            },
            None,
        )
        .await;
}

/// Read-only: is `code` a live match (past Waiting)? A missing / undecodable
/// session, or one that's `Ended`, counts as not active.
pub(super) async fn session_status_is_active(state: &AppState, code: &str) -> bool {
    let Ok(mut conn) = state.redis.get().await else {
        return false;
    };
    let raw: Option<String> = conn.get(format!("session:{}", code)).await.unwrap_or(None);
    let Some(raw) = raw else {
        return false;
    };
    matches!(
        serde_json::from_str::<Session>(&raw).map(|s| s.status),
        Ok(SessionStatus::Starting) | Ok(SessionStatus::Active)
    )
}

/// Arm a dropped mid-game host's grace: a 30s `Promotion` timer (promote the
/// next player, demoting this host into joiners so they keep their window) AND
/// the shared 2-min `Reconnect` slot-hold (below). Announces `HostReconnecting`
/// so peers show the overlay. The reconnect path takes whichever entry applies,
/// waking the relevant timer task early.
pub(super) async fn arm_host_disconnect_grace(state: &AppState, code: &str, host_id: &str) {
    let Some(promo_rx) = state.signal_hub.arm_grace(code, host_id, GraceKind::Promotion).await
    else {
        // The host already has a live socket (it reconnected on a new connection
        // before this stale disconnect path ran) — don't arm or announce grace
        // against a host that's actually present.
        tracing::info!("ws: host {} already present in session {}, grace not armed", host_id, code);
        return;
    };

    state
        .signal_hub
        .broadcast(
            code,
            ServerMsg::HostReconnecting {
                player_id: host_id.to_string(),
                grace_secs: PROMOTION_GRACE.as_secs(),
            },
            None,
        )
        .await;
    tracing::info!("ws: host {} dropped from active session {} — {}s promotion grace, {}s rejoin hold",
        host_id, code, PROMOTION_GRACE.as_secs(), RECONNECT_GRACE.as_secs());

    // Promotion timer: at PROMOTION_GRACE, claim the entry (single-winner vs the
    // host reconnecting) and promote the next player, keeping the ex-host as a
    // demoted joiner so they can still rejoin within the reconnect hold.
    {
        let state = state.clone();
        let code = code.to_string();
        let host_id = host_id.to_string();
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(PROMOTION_GRACE) => {
                    if state.signal_hub.take_grace(&code, &host_id, GraceKind::Promotion).await {
                        if let Err(e) = promote_demote_or_end_active(&state, &code, &host_id, true).await {
                            tracing::warn!("ws: host-grace promotion failed for {}: {}", code, e);
                        }
                    }
                }
                _ = promo_rx => { /* host re-Identified before promotion */ }
            }
        });
    }

    // Reconnect slot-hold for the (possibly demoted) ex-host.
    arm_reconnect_slot_hold(state, code, host_id.to_string()).await;
}

/// Arm a dropped mid-game joiner's reconnect window: announce `PeerReconnecting`
/// (peers show the overlay) and start the shared 2-min slot-hold. No promotion
/// timer — joiners aren't host candidates.
pub(super) async fn arm_joiner_disconnect_grace(state: &AppState, code: &str, player_id: &str) {
    // Arm the hold + spawn the timer; only announce the overlay when a hold was
    // actually armed (false ⇒ the joiner already reconnected on a new socket).
    if arm_reconnect_slot_hold(state, code, player_id.to_string()).await {
        state
            .signal_hub
            .broadcast(
                code,
                ServerMsg::PeerReconnecting {
                    player_id: player_id.to_string(),
                    grace_secs: PROMOTION_GRACE.as_secs(),
                },
                None,
            )
            .await;
        tracing::info!("ws: joiner {} dropped from active session {} — {}s rejoin hold",
            player_id, code, RECONNECT_GRACE.as_secs());
    }
}

/// Arm (if not already) the 2-min `Reconnect` slot-hold for `player_id` and spawn
/// the timer that frees their slot when it elapses. Returns `true` if a hold is
/// now pending (false if the player already has a live socket, so nothing to
/// hold). Shared by the host and joiner disconnect paths.
async fn arm_reconnect_slot_hold(state: &AppState, code: &str, player_id: String) -> bool {
    let Some(recon_rx) = state
        .signal_hub
        .arm_grace(code, &player_id, GraceKind::Reconnect)
        .await
    else {
        return false;
    };

    let state = state.clone();
    let code = code.to_string();
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(RECONNECT_GRACE) => {
                // Window elapsed: claim it (single-winner vs a late reconnect)
                // and free the slot for good.
                if state.signal_hub.take_grace(&code, &player_id, GraceKind::Reconnect).await {
                    free_member_after_timeout(&state, &code, &player_id).await;
                }
            }
            _ = recon_rx => { /* player rejoined within the window */ }
        }
    });
    true
}

/// A held slot's rejoin window elapsed: tell peers the player is gone for good
/// and free their Redis slot (reusing the explicit-leave removal, which also
/// ends the session if this leaves the host alone).
async fn free_member_after_timeout(state: &AppState, code: &str, player_id: &str) {
    state
        .signal_hub
        .broadcast(
            code,
            ServerMsg::PeerLeft { player_id: player_id.to_string(), reason: "reconnect_timeout" },
            None,
        )
        .await;
    if let Err(e) = remove_joiner_on_leave(state, code, player_id).await {
        tracing::warn!("ws: reconnect-timeout cleanup failed for {} in {}: {}", player_id, code, e);
    }
    tracing::info!("ws: player {} rejoin window elapsed in session {} — slot freed", player_id, code);
}
