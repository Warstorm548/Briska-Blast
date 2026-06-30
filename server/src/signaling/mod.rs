pub mod protocol;
pub mod ws;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, oneshot, RwLock};

pub use protocol::ServerMsg;

/// Ephemeral in-process registry of WebSocket signaling rooms keyed by
/// session code. Each room maps player_id → (connection_id, mpsc sender)
/// so the hub can fan out broadcasts or push targeted messages without
/// the WS handlers knowing about each other.
///
/// State is intentionally not persisted to Redis: a server restart drops
/// every WS anyway, and rebuilding the registry across instances would
/// require cross-process pub/sub for what a single-instance deployment
/// can do entirely in memory.
#[derive(Default)]
pub struct SignalHub {
    rooms: RwLock<HashMap<String, Room>>,
    next_conn_id: AtomicU64,
    /// Pending grace timers, keyed by `(session_code, player_id, kind)`. When a
    /// player's WS drops mid-game, the disconnect path arms a timer here; the
    /// player re-Identifying (or the timer firing) "takes" the entry. Dropping
    /// the stored sender wakes the waiting timer task so it exits promptly on
    /// reconnect. Two independent kinds can be pending for the same player at
    /// once — a dropped host has both a 30s `Promotion` timer and a 2-min
    /// `Reconnect` slot-hold — so neither cancels the other. Ephemeral, like the
    /// rooms map.
    grace: RwLock<HashMap<(String, String, GraceKind), oneshot::Sender<()>>>,
}

/// Which grace window an entry represents (see [`SignalHub::grace`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraceKind {
    /// Host-only: the 30s deadline before the next player is promoted to host.
    Promotion,
    /// Any dropped player: the longer slot-hold during which they may rejoin
    /// before their session slot is freed.
    Reconnect,
}

#[derive(Default)]
struct Room {
    /// player_id → (connection_id, sender). The connection_id lets
    /// `leave_room` distinguish a stale cleanup (old socket finishing
    /// its disconnect path) from a real eviction. If a player_id has
    /// already been re-bound to a newer connection, the old socket's
    /// cleanup must not remove the newer sender.
    senders: HashMap<String, (u64, mpsc::UnboundedSender<ServerMsg>)>,
    /// Authoritative per-session score tally (player_id → points). Lives
    /// for the room's lifetime; not persisted to Redis (same rationale as
    /// the senders map above). Cleared implicitly when the room is dropped
    /// once empty.
    scores: HashMap<String, i64>,
    /// The score that ends the match (the win condition's target), seeded at
    /// `/start` via [`SignalHub::set_win_target`]. 0 means "no limit / unset"
    /// (win detection is skipped). In-memory like `scores`, so a server restart
    /// drops it along with the live match.
    win_target: i64,
    /// Latches true the instant the target is first reached, so a late or
    /// duplicate `ReportScore` after the win can't fire a second `GameOver`.
    game_over: bool,
}

impl SignalHub {
    /// Register a sender for `player_id` in `code`'s room. Returns the
    /// connection_id (used later to scope `leave_room` correctly) and
    /// the receiving end (the WS handler pumps outbound messages from
    /// it). Creates the room if it doesn't exist.
    ///
    /// If the same `player_id` was already registered (a duplicate
    /// identify on a new socket), the older entry is replaced — its
    /// sender is dropped, which closes the old receiver and naturally
    /// breaks the old WS handler's pump loop.
    pub async fn join_room(
        &self,
        code: &str,
        player_id: &str,
    ) -> (u64, mpsc::UnboundedReceiver<ServerMsg>) {
        let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::unbounded_channel();
        let mut rooms = self.rooms.write().await;
        let room = rooms.entry(code.to_string()).or_default();
        room.senders.insert(player_id.to_string(), (conn_id, tx));
        (conn_id, rx)
    }

    /// Remove `player_id` from `code`'s room IF the stored entry matches
    /// the given `conn_id`. The match check prevents an old WS handler's
    /// late cleanup from evicting a newer socket that already re-bound
    /// the same player_id (the duplicate-identify race).
    ///
    /// Drops the room entirely once the last sender leaves, so the map
    /// doesn't accumulate empty rooms forever as sessions end.
    pub async fn leave_room(&self, code: &str, player_id: &str, conn_id: u64) {
        let mut rooms = self.rooms.write().await;
        let Some(room) = rooms.get_mut(code) else { return };
        if room.senders.get(player_id).map(|(id, _)| *id) == Some(conn_id) {
            room.senders.remove(player_id);
        }
        if room.senders.is_empty() {
            rooms.remove(code);
        }
    }

    /// Push a message to a single player. Returns true if delivered
    /// (sender exists and channel still open), false otherwise.
    pub async fn send_to(&self, code: &str, player_id: &str, msg: ServerMsg) -> bool {
        let rooms = self.rooms.read().await;
        rooms
            .get(code)
            .and_then(|r| r.senders.get(player_id))
            .map(|(_, tx)| tx.send(msg).is_ok())
            .unwrap_or(false)
    }

    /// Broadcast a message to every player in the room. If `except` is
    /// `Some(player_id)`, that sender is skipped (used so a peer doesn't
    /// receive an echo of their own announcement).
    pub async fn broadcast(&self, code: &str, msg: ServerMsg, except: Option<&str>) {
        let rooms = self.rooms.read().await;
        if let Some(room) = rooms.get(code) {
            for (pid, (_, tx)) in &room.senders {
                if Some(pid.as_str()) == except {
                    continue;
                }
                let _ = tx.send(msg.clone());
            }
        }
    }

    /// Seed (or reseed) the win target for `code`'s room — the score that ends the
    /// match. Called by `/start`. A target of 0 disables win detection. Clears any
    /// stale `game_over` latch. Creates the room if it somehow doesn't exist yet
    /// (it normally does: every member identified before `/start`).
    pub async fn set_win_target(&self, code: &str, target: i64) {
        let mut rooms = self.rooms.write().await;
        let room = rooms.entry(code.to_string()).or_default();
        room.win_target = target;
        room.game_over = false;
    }

    /// Max points a single score report may credit (master ball = 1, BallBT split
    /// ball = 2). Clamps a forged/garbled report.
    const MAX_SCORE_POINTS: i64 = 2;

    /// Credit `scoring_player_id` its `points` in `code`'s room and return the full
    /// updated tally to broadcast, plus the winner's id iff this point first met
    /// the win target. Returns `None` if the room doesn't exist (e.g. the session
    /// already ended) so the caller skips the broadcast. Win detection latches via
    /// `game_over`, so a late/duplicate report after the win updates the tally but
    /// never re-announces a winner. Idempotency of a single report is the caller's
    /// concern — each accepted `ReportScore` credits its (clamped) `points`.
    pub async fn record_score(
        &self,
        code: &str,
        scoring_player_id: &str,
        points: i64,
    ) -> Option<(HashMap<String, i64>, Option<String>)> {
        let mut rooms = self.rooms.write().await;
        let room = rooms.get_mut(code)?;
        // Only credit players currently in the room — don't let a report mint a
        // tally entry for an arbitrary / departed id. (Trajectory validation of
        // *who* scored remains the later hook.)
        if !room.senders.contains_key(scoring_player_id) {
            return None;
        }
        // Clamp so a forged/garbled report can't mint an arbitrary number of
        // points (master ball = 1, BallBT split ball = 2).
        let points = points.clamp(1, Self::MAX_SCORE_POINTS);
        let new_total = {
            let entry = room.scores.entry(scoring_player_id.to_string()).or_insert(0);
            *entry += points;
            *entry
        };
        let winner = if !room.game_over && room.win_target > 0 && new_total >= room.win_target {
            room.game_over = true;
            Some(scoring_player_id.to_string())
        } else {
            None
        };
        Some((room.scores.clone(), winner))
    }

    /// Snapshot of player_ids currently identified in the room. Used by
    /// `/start` to check that every session member has a live WS before
    /// transitioning to the signaling phase.
    pub async fn room_members(&self, code: &str) -> Vec<String> {
        let rooms = self.rooms.read().await;
        rooms
            .get(code)
            .map(|r| r.senders.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Arm a grace timer of `kind` for `(code, player_id)` and return the
    /// receiver the timer task selects on — or `None` if the player already has
    /// a live socket in the room, in which case grace must **not** be armed (a
    /// stale disconnect path, e.g. a half-open old socket cleaned up after the
    /// player already reconnected on a new connection, would otherwise act
    /// against a player that is actually present). The returned receiver
    /// resolves (with an error) as soon as the stored sender is dropped — i.e.
    /// when `take_grace` removes the entry — so the task can stop waiting and
    /// exit the moment the player reconnects. Overwrites any existing entry for
    /// the same key (which cancels the older timer).
    pub async fn arm_grace(
        &self,
        code: &str,
        player_id: &str,
        kind: GraceKind,
    ) -> Option<oneshot::Receiver<()>> {
        if self
            .rooms
            .read()
            .await
            .get(code)
            .is_some_and(|r| r.senders.contains_key(player_id))
        {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        let mut grace = self.grace.write().await;
        grace.insert((code.to_string(), player_id.to_string(), kind), tx);
        Some(rx)
    }

    /// Remove the `kind` grace entry for `(code, player_id)` and report whether
    /// one was pending. This is the single-winner handoff between the two ways a
    /// grace window ends: the player re-Identifying calls this to *cancel* (true
    /// → it was still pending), and the timer firing calls this to *claim* (true
    /// → it won the race, so act). Whichever runs first gets `true`; the loser
    /// gets `false` and does nothing, so a timer can never double-fire against a
    /// reconnect. The two kinds are independent — taking one leaves the other.
    pub async fn take_grace(&self, code: &str, player_id: &str, kind: GraceKind) -> bool {
        let mut grace = self.grace.write().await;
        grace
            .remove(&(code.to_string(), player_id.to_string(), kind))
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn join_room_returns_a_receiver_and_registers_the_sender() {
        let hub = SignalHub::default();
        let (_conn_id, _rx) = hub.join_room("ABC", "0000001").await;
        let members = hub.room_members("ABC").await;
        assert_eq!(members, vec!["0000001".to_string()]);
    }

    #[tokio::test]
    async fn send_to_delivers_only_to_the_named_player() {
        let hub = SignalHub::default();
        let (_, mut rx_a) = hub.join_room("ABC", "0000001").await;
        let (_, mut rx_b) = hub.join_room("ABC", "0000002").await;

        let delivered = hub
            .send_to("ABC", "0000002", ServerMsg::Kicked { reason: "test" })
            .await;
        assert!(delivered);

        assert!(rx_a.try_recv().is_err()); // A got nothing
        assert!(matches!(rx_b.try_recv(), Ok(ServerMsg::Kicked { .. })));
    }

    #[tokio::test]
    async fn send_to_missing_player_returns_false() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;
        let delivered = hub
            .send_to("ABC", "0000099", ServerMsg::Kicked { reason: "test" })
            .await;
        assert!(!delivered);
    }

    #[tokio::test]
    async fn broadcast_reaches_everyone_when_except_is_none() {
        let hub = SignalHub::default();
        let (_, mut rx_a) = hub.join_room("ABC", "0000001").await;
        let (_, mut rx_b) = hub.join_room("ABC", "0000002").await;

        hub.broadcast(
            "ABC",
            ServerMsg::PeerJoined {
                player_id: "X".into(),
                username: "Xander".into(),
            },
            None,
        )
        .await;

        assert!(matches!(rx_a.try_recv(), Ok(ServerMsg::PeerJoined { .. })));
        assert!(matches!(rx_b.try_recv(), Ok(ServerMsg::PeerJoined { .. })));
    }

    #[tokio::test]
    async fn broadcast_skips_the_excepted_player() {
        let hub = SignalHub::default();
        let (_, mut rx_a) = hub.join_room("ABC", "0000001").await;
        let (_, mut rx_b) = hub.join_room("ABC", "0000002").await;

        hub.broadcast(
            "ABC",
            ServerMsg::PeerJoined {
                player_id: "X".into(),
                username: "Xander".into(),
            },
            Some("0000001"),
        )
        .await;

        assert!(rx_a.try_recv().is_err()); // sender excluded
        assert!(matches!(rx_b.try_recv(), Ok(ServerMsg::PeerJoined { .. })));
    }

    #[tokio::test]
    async fn leave_room_drops_empty_room() {
        let hub = SignalHub::default();
        let (conn_id, _rx) = hub.join_room("ABC", "0000001").await;
        hub.leave_room("ABC", "0000001", conn_id).await;
        assert!(hub.room_members("ABC").await.is_empty());
        let delivered = hub
            .send_to("ABC", "0000001", ServerMsg::Kicked { reason: "test" })
            .await;
        assert!(!delivered);
    }

    #[tokio::test]
    async fn room_members_returns_empty_for_unknown_room() {
        let hub = SignalHub::default();
        assert!(hub.room_members("NOPE").await.is_empty());
    }

    #[tokio::test]
    async fn concurrent_joins_to_distinct_players_both_register() {
        use std::sync::Arc;
        let hub = Arc::new(SignalHub::default());

        let h1 = {
            let hub = hub.clone();
            tokio::spawn(async move {
                let (_, _rx) = hub.join_room("ABC", "0000001").await;
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            })
        };
        let h2 = {
            let hub = hub.clone();
            tokio::spawn(async move {
                let (_, _rx) = hub.join_room("ABC", "0000002").await;
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            })
        };
        h1.await.unwrap();
        h2.await.unwrap();

        let mut members = hub.room_members("ABC").await;
        members.sort();
        assert_eq!(members, vec!["0000001".to_string(), "0000002".to_string()]);
    }

    #[tokio::test]
    async fn stale_leave_does_not_evict_newer_connection_for_same_player() {
        // Scenario: socket A1 registers player 0000001. Socket A2 reconnects
        // (same player_id, duplicate identify) and overwrites A1's entry.
        // A1's disconnect cleanup then runs and calls leave_room — it must
        // NOT remove A2's entry. The nonce makes this work.
        let hub = SignalHub::default();
        let (old_conn, _rx_old) = hub.join_room("ABC", "0000001").await;
        let (_new_conn, mut rx_new) = hub.join_room("ABC", "0000001").await;

        // A1's late cleanup with the old conn_id.
        hub.leave_room("ABC", "0000001", old_conn).await;

        // A2 must still be registered.
        assert_eq!(hub.room_members("ABC").await, vec!["0000001".to_string()]);
        let delivered = hub
            .send_to("ABC", "0000001", ServerMsg::Kicked { reason: "ok" })
            .await;
        assert!(delivered);
        assert!(matches!(rx_new.try_recv(), Ok(ServerMsg::Kicked { .. })));
    }

    #[tokio::test]
    async fn record_score_increments_and_returns_the_tally() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;

        let (tally, winner) = hub.record_score("ABC", "0000001", 1).await.unwrap();
        assert_eq!(tally.get("0000001"), Some(&1));
        // No win target set → never a winner.
        assert!(winner.is_none());
    }

    #[tokio::test]
    async fn record_score_accumulates_across_players() {
        let hub = SignalHub::default();
        let (_, _rx1) = hub.join_room("ABC", "0000001").await;
        let (_, _rx2) = hub.join_room("ABC", "0000002").await;

        hub.record_score("ABC", "0000001", 1).await.unwrap();
        hub.record_score("ABC", "0000002", 1).await.unwrap();
        let (tally, _) = hub.record_score("ABC", "0000001", 1).await.unwrap();

        assert_eq!(tally.get("0000001"), Some(&2));
        assert_eq!(tally.get("0000002"), Some(&1));
    }

    #[tokio::test]
    async fn record_score_for_non_member_returns_none() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;
        // 0000002 never joined — its report mints no tally entry.
        assert!(hub.record_score("ABC", "0000002", 1).await.is_none());
        let (tally, _) = hub.record_score("ABC", "0000001", 1).await.unwrap();
        assert!(tally.get("0000002").is_none());
    }

    #[tokio::test]
    async fn record_score_for_unknown_room_returns_none() {
        let hub = SignalHub::default();
        assert!(hub.record_score("NOPE", "0000001", 1).await.is_none());
    }

    #[tokio::test]
    async fn record_score_announces_winner_when_target_reached() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;
        hub.set_win_target("ABC", 2).await;

        // First point: below target, no winner.
        let (_, winner) = hub.record_score("ABC", "0000001", 1).await.unwrap();
        assert!(winner.is_none());

        // Second point: meets target → winner announced once.
        let (tally, winner) = hub.record_score("ABC", "0000001", 1).await.unwrap();
        assert_eq!(tally.get("0000001"), Some(&2));
        assert_eq!(winner.as_deref(), Some("0000001"));
    }

    #[tokio::test]
    async fn record_score_announces_winner_only_once() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;
        hub.set_win_target("ABC", 1).await;

        let (_, winner) = hub.record_score("ABC", "0000001", 1).await.unwrap();
        assert_eq!(winner.as_deref(), Some("0000001"));

        // A late report after game over still tallies but never re-announces.
        let (tally, winner) = hub.record_score("ABC", "0000001", 1).await.unwrap();
        assert_eq!(tally.get("0000001"), Some(&2));
        assert!(winner.is_none());
    }

    #[tokio::test]
    async fn record_score_credits_multiple_points() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;

        // A BallBT split ball is worth 2 in a single report.
        let (tally, _) = hub.record_score("ABC", "0000001", 2).await.unwrap();
        assert_eq!(tally.get("0000001"), Some(&2));
    }

    #[tokio::test]
    async fn record_score_clamps_points_to_range() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;

        // A forged out-of-range report is clamped down to the max (2).
        let (tally, _) = hub.record_score("ABC", "0000001", 99).await.unwrap();
        assert_eq!(tally.get("0000001"), Some(&2));
        // ...and a zero/negative report still credits at least 1.
        let (tally, _) = hub.record_score("ABC", "0000001", 0).await.unwrap();
        assert_eq!(tally.get("0000001"), Some(&3));
    }

    #[tokio::test]
    async fn record_score_double_can_reach_target_in_one_report() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;
        hub.set_win_target("ABC", 2).await;

        // One double-point goal meets a target of 2 immediately.
        let (_, winner) = hub.record_score("ABC", "0000001", 2).await.unwrap();
        assert_eq!(winner.as_deref(), Some("0000001"));
    }

    #[tokio::test]
    async fn arm_then_take_grace_returns_true_once() {
        let hub = SignalHub::default();
        let _rx = hub.arm_grace("ABC", "0000001", GraceKind::Promotion).await;
        // First take claims the entry; a second take finds nothing.
        assert!(hub.take_grace("ABC", "0000001", GraceKind::Promotion).await);
        assert!(!hub.take_grace("ABC", "0000001", GraceKind::Promotion).await);
    }

    #[tokio::test]
    async fn take_grace_unknown_key_is_false() {
        let hub = SignalHub::default();
        assert!(!hub.take_grace("ABC", "0000001", GraceKind::Promotion).await);
        // A grace armed for one player is not taken by a different player id.
        let _rx = hub.arm_grace("ABC", "0000001", GraceKind::Promotion).await;
        assert!(!hub.take_grace("ABC", "0000002", GraceKind::Promotion).await);
    }

    #[tokio::test]
    async fn the_two_grace_kinds_are_independent() {
        // A dropped host has both a Promotion and a Reconnect timer pending for
        // the same key; taking one must leave the other intact.
        let hub = SignalHub::default();
        let _p = hub.arm_grace("ABC", "0000001", GraceKind::Promotion).await;
        let _r = hub.arm_grace("ABC", "0000001", GraceKind::Reconnect).await;
        assert!(hub.take_grace("ABC", "0000001", GraceKind::Promotion).await);
        // Reconnect is untouched by taking Promotion.
        assert!(hub.take_grace("ABC", "0000001", GraceKind::Reconnect).await);
    }

    #[tokio::test]
    async fn taking_grace_wakes_the_waiting_receiver() {
        // The timer task selects on this receiver so it can exit early when the
        // player reconnects. take_grace drops the sender, which must resolve the
        // receiver (with an error) rather than leave it pending forever.
        let hub = SignalHub::default();
        let rx = hub
            .arm_grace("ABC", "0000001", GraceKind::Reconnect)
            .await
            .expect("armed");
        assert!(hub.take_grace("ABC", "0000001", GraceKind::Reconnect).await);
        assert!(rx.await.is_err());
    }

    #[tokio::test]
    async fn arm_grace_skips_when_player_still_connected() {
        // A stale disconnect path must not arm a timer for a player that already
        // has a live socket (reconnected on a new connection).
        let hub = SignalHub::default();
        let (_conn, _rx) = hub.join_room("ABC", "0000001").await;
        assert!(hub
            .arm_grace("ABC", "0000001", GraceKind::Promotion)
            .await
            .is_none());
        // Nothing was armed, so a later take finds no entry.
        assert!(!hub.take_grace("ABC", "0000001", GraceKind::Promotion).await);
    }
}
