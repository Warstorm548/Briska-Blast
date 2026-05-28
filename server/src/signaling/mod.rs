pub mod protocol;
pub mod ws;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, RwLock};

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

    /// Credit `scoring_player_id` one point in `code`'s room and return the
    /// full updated tally to broadcast. Returns `None` if the room doesn't
    /// exist (e.g. the session already ended) so the caller skips the
    /// broadcast. Idempotency is the caller's concern — each accepted
    /// `ReportScore` is one point.
    pub async fn record_score(
        &self,
        code: &str,
        scoring_player_id: &str,
    ) -> Option<HashMap<String, i64>> {
        let mut rooms = self.rooms.write().await;
        let room = rooms.get_mut(code)?;
        *room.scores.entry(scoring_player_id.to_string()).or_insert(0) += 1;
        Some(room.scores.clone())
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

        hub.broadcast("ABC", ServerMsg::PeerJoined { player_id: "X".into() }, None)
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
            ServerMsg::PeerJoined { player_id: "X".into() },
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

        let tally = hub.record_score("ABC", "0000001").await.unwrap();
        assert_eq!(tally.get("0000001"), Some(&1));
    }

    #[tokio::test]
    async fn record_score_accumulates_across_players() {
        let hub = SignalHub::default();
        let (_, _rx) = hub.join_room("ABC", "0000001").await;

        hub.record_score("ABC", "0000001").await.unwrap();
        hub.record_score("ABC", "0000002").await.unwrap();
        let tally = hub.record_score("ABC", "0000001").await.unwrap();

        assert_eq!(tally.get("0000001"), Some(&2));
        assert_eq!(tally.get("0000002"), Some(&1));
    }

    #[tokio::test]
    async fn record_score_for_unknown_room_returns_none() {
        let hub = SignalHub::default();
        assert!(hub.record_score("NOPE", "0000001").await.is_none());
    }
}
