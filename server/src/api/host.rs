use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use rand::Rng;
use shared::protocol::messages::{HostRequest, HostResponse};
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, validate_player, Session};

const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

fn generate_code() -> String {
    let mut rng = rand::thread_rng();
    (0..6)
        .map(|_| CODE_ALPHABET[rng.gen_range(0..CODE_ALPHABET.len())] as char)
        .collect()
}

pub async fn host(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<HostRequest>,
) -> Result<Json<HostResponse>> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_host.check_key(&ip).is_err() {
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    validate_player(&mut conn, &body.player_id, &body.secret_token).await?;

    let session_code = loop {
        let code = generate_code();
        let exists: bool = conn
            .exists(format!("session:{}", code))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if !exists {
            break code;
        }
    };

    let session = Session {
        code: session_code.clone(),
        host_player_id: body.player_id.clone(),
        host_ip: body.external_ip,
        host_port: body.external_port,
        gamemode: body.gamemode.clone(),
        joiner_player_id: None,
        joiner_ip: None,
        joiner_port: None,
        status: "waiting".to_string(),
    };

    let json = serde_json::to_string(&session)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    conn.set_ex::<_, _, ()>(
        format!("session:{}", session_code),
        json,
        state.config.session_ttl_secs,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!(
        "player {} created session {}",
        body.player_id, session_code
    );

    Ok(Json(HostResponse { session_code }))
}
