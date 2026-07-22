//! Admin **Logs** tab — tails this deployment's container stdout/stderr via the
//! already-mounted docker socket (`bollard`, same client used by the update
//! flow in `update/docker.rs`).
//!
//! Two invariants:
//!   1. **Every** line is run through [`crate::redact::redact_ips`] before it
//!      leaves the handler, so a raw client IP printed by redis/watchtower/etc.
//!      can never reach the browser or a downloaded `.log`.
//!   2. Access is per **source**, not one blanket gate: [`SOURCES`] maps each
//!      readable source to a minimum [`AdminRole`]. This is the whole
//!      "selective per-group access" story — a future source (e.g. a
//!      session-chat log for Moderators) is one table row with its own
//!      `min_role`, and the nav gate, the `<select>`, and the per-request check
//!      all follow automatically.

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap},
    response::{Html, IntoResponse, Redirect, Response},
};
use bollard::container::{ListContainersOptions, LogOutput, LogsOptions};
use bollard::Docker;
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;

use super::{require_session, templates, AdminRole, AdminSession, FORBIDDEN_REDIRECT};
use crate::redact::redact_ips;
use crate::state::AppState;

/// A readable log source: its url key, display label, the docker-compose
/// service to read, and the minimum role allowed to read it.
pub struct SourceSpec {
    pub key: &'static str,
    pub label: &'static str,
    service: &'static str,
    min_role: AdminRole,
}

/// The access policy. v1: the three deployment containers, all Admin+.
const SOURCES: &[SourceSpec] = &[
    SourceSpec { key: "server", label: "Server", service: "server", min_role: AdminRole::Admin },
    SourceSpec { key: "redis", label: "Redis", service: "redis", min_role: AdminRole::Admin },
    SourceSpec { key: "watchtower", label: "Watchtower", service: "watchtower", min_role: AdminRole::Admin },
];

/// Allowed tail sizes; anything else falls back to the default.
const LINE_CHOICES: &[u32] = &[100, 500, 2000];
const DEFAULT_LINES: u32 = 500;

fn source_spec(key: &str) -> Option<&'static SourceSpec> {
    SOURCES.iter().find(|s| s.key == key)
}

/// The (key, label) sources this role may read — drives the `<select>`.
pub fn readable_sources(role: AdminRole) -> Vec<(&'static str, &'static str)> {
    SOURCES
        .iter()
        .filter(|s| role.at_least(s.min_role))
        .map(|s| (s.key, s.label))
        .collect()
}

/// Whether the Logs nav link should show for this role (can read ≥1 source).
pub fn can_view_logs(role: AdminRole) -> bool {
    SOURCES.iter().any(|s| role.at_least(s.min_role))
}

fn clamp_lines(n: Option<u32>) -> u32 {
    match n {
        Some(v) if LINE_CHOICES.contains(&v) => v,
        _ => DEFAULT_LINES,
    }
}

#[derive(Deserialize)]
pub struct LogsQuery {
    source: Option<String>,
    lines: Option<u32>,
}

/// Authorize an already-validated session against a *specific* source. Unknown
/// source or a role below the source's floor is refused. Synchronous — the
/// caller fetches the session once.
fn authorize_source(
    session: &AdminSession,
    source_key: &str,
) -> Result<&'static SourceSpec, Redirect> {
    let spec = source_spec(source_key)
        .ok_or_else(|| Redirect::to("/admin/logs?err=Unknown+log+source"))?;
    if !session.at_least(spec.min_role) {
        return Err(Redirect::to(FORBIDDEN_REDIRECT));
    }
    Ok(spec)
}

/// GET /admin/logs — the tab shell with an initial redacted tail.
pub async fn logs_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LogsQuery>,
) -> Response {
    // Must be signed in to resolve a default source / render the nav.
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let sources = readable_sources(session.role);
    if sources.is_empty() {
        // No readable sources for this role — bounce like any forbidden page.
        return Redirect::to(FORBIDDEN_REDIRECT).into_response();
    }
    // Default to the caller's first readable source when none is requested.
    let source_key = q.source.clone().unwrap_or_else(|| sources[0].0.to_string());
    let spec = match authorize_source(&session, &source_key) {
        Ok(s) => s,
        Err(redirect) => return redirect.into_response(),
    };
    let lines = clamp_lines(q.lines);

    let (body, error) = match fetch_logs(&state, spec, lines).await {
        Ok(text) => (text, None),
        Err(e) => {
            tracing::warn!(source = spec.key, error = %e, "logs fetch failed");
            (String::new(), Some(e))
        }
    };

    Html(templates::logs_page(
        &sources,
        spec.key,
        lines,
        LINE_CHOICES,
        &body,
        error.as_deref(),
        session.role,
        &session.username,
    ))
    .into_response()
}

/// GET /admin/logs/data — redacted plain-text tail, for the Refresh/auto-poll.
pub async fn logs_data(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LogsQuery>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let source_key = q.source.clone().unwrap_or_default();
    let spec = match authorize_source(&session, &source_key) {
        Ok(s) => s,
        Err(redirect) => return redirect.into_response(),
    };
    let lines = clamp_lines(q.lines);
    let body = match fetch_logs(&state, spec, lines).await {
        Ok(text) => text,
        Err(e) => format!("\u{26a0} {e}"),
    };
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string())], body).into_response()
}

/// GET /admin/logs/download — same redacted tail as an attachment.
pub async fn logs_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LogsQuery>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let source_key = q.source.clone().unwrap_or_default();
    let spec = match authorize_source(&session, &source_key) {
        Ok(s) => s,
        Err(redirect) => return redirect.into_response(),
    };
    let lines = clamp_lines(q.lines);
    let body = match fetch_logs(&state, spec, lines).await {
        Ok(text) => text,
        Err(e) => format!("\u{26a0} {e}"),
    };
    let filename = format!(
        "briska_{}_{}.log",
        spec.key,
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response()
}

/// Read up to `lines` of `spec`'s container log, demux stdout+stderr, and redact
/// every IP before returning. Never surfaces a raw docker error to the browser
/// beyond a short message.
async fn fetch_logs(state: &AppState, spec: &SourceSpec, lines: u32) -> Result<String, String> {
    let docker =
        Docker::connect_with_socket_defaults().map_err(|e| format!("docker connect: {e}"))?;
    let project = own_compose_project(&docker).await;
    let id = find_container(&docker, project.as_deref(), spec.service)
        .await
        .ok_or_else(|| format!("no running '{}' container found", spec.service))?;

    let opts = LogsOptions::<String> {
        stdout: true,
        stderr: true,
        timestamps: true,
        tail: lines.to_string(),
        ..Default::default()
    };
    let mut stream = docker.logs(&id, Some(opts));
    let mut buf = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(LogOutput::StdOut { message })
            | Ok(LogOutput::StdErr { message })
            | Ok(LogOutput::Console { message })
            | Ok(LogOutput::StdIn { message }) => {
                buf.push_str(&String::from_utf8_lossy(&message));
            }
            Err(e) => return Err(format!("read logs: {e}")),
        }
    }
    Ok(redact_ips(&state.ip_hash_key, &buf))
}

/// This server's docker-compose project, read from its own container labels —
/// so a multi-channel host (dev+ea+stable side by side) never cross-reads
/// another deployment's containers. `None` when not compose-managed.
async fn own_compose_project(docker: &Docker) -> Option<String> {
    let hostname = std::env::var("HOSTNAME").ok()?;
    let info = docker.inspect_container(&hostname, None).await.ok()?;
    info.config?
        .labels?
        .get("com.docker.compose.project")
        .cloned()
}

/// Find the container id for a compose `service`, scoped to `project` when
/// known. Falls back to service-label-only match (bare/dev runs).
async fn find_container(docker: &Docker, project: Option<&str>, service: &str) -> Option<String> {
    let mut labels = vec![format!("com.docker.compose.service={service}")];
    if let Some(p) = project {
        labels.push(format!("com.docker.compose.project={p}"));
    }
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), labels);
    let opts = ListContainersOptions::<String> {
        all: true,
        filters,
        ..Default::default()
    };
    let list = docker.list_containers(Some(opts)).await.ok()?;
    list.into_iter().find_map(|c| c.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moderator_sees_no_sources_admin_sees_all() {
        assert!(readable_sources(AdminRole::Moderator).is_empty());
        assert!(!can_view_logs(AdminRole::Moderator));
        assert_eq!(readable_sources(AdminRole::Admin).len(), SOURCES.len());
        assert_eq!(readable_sources(AdminRole::SuperAdmin).len(), SOURCES.len());
        assert!(can_view_logs(AdminRole::Admin));
    }

    #[test]
    fn line_count_is_clamped_to_choices() {
        assert_eq!(clamp_lines(Some(100)), 100);
        assert_eq!(clamp_lines(Some(2000)), 2000);
        assert_eq!(clamp_lines(Some(9999)), DEFAULT_LINES);
        assert_eq!(clamp_lines(None), DEFAULT_LINES);
    }

    #[test]
    fn unknown_source_is_rejected() {
        assert!(source_spec("../etc/passwd").is_none());
        assert!(source_spec("server").is_some());
    }
}
