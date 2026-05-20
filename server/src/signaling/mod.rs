pub mod protocol;
pub mod ws;

use std::collections::HashMap;
use tokio::sync::{mpsc, RwLock};

pub use protocol::ServerMsg;

/// Ephemeral in-process registry of WebSocket signaling rooms keyed by
/// session code. Each room maps player_id → mpsc sender so the hub can
/// fan out broadcasts or push targeted messages without the WS handlers
/// knowing about each other.
///
/// State is intentionally not persisted to Redis: a server restart drops
/// every WS anyway, and rebuilding the registry across instances would
/// require cross-process pub/sub for what a single-instance deployment
/// can do entirely in memory.
#[derive(Default)]
pub struct SignalHub {
    rooms: RwLock<HashMap<String, Room>>,
}

#[derive(Default)]
struct Room {
    senders: HashMap<String, mpsc::UnboundedSender<ServerMsg>>,
}

impl SignalHub {
    /// Register a sender for `player_id` in `code`'s room. Returns the
    /// receiving end so the caller (the WS handler) can pump outbound
    /// messages into the socket. Creates the room if it doesn't exist.
    pub async fn join_room(
        &self,
        code: &str,
        player_id: &str,
    ) -> mpsc::UnboundedReceiver<ServerMsg> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut rooms = self.rooms.write().await;
        let room = rooms.entry(code.to_string()).or_default();
        room.senders.insert(player_id.to_string(), tx);
        rx
    }

    /// Remove `player_id` from `code`'s room. Drops the room entirely if
    /// the last sender just left, so the map doesn't accumulate empty
    /// rooms forever as sessions end.
    pub async fn leave_room(&self, code: &str, player_id: &str) {
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get_mut(code) {
            room.senders.remove(player_id);
            if room.senders.is_empty() {
                rooms.remove(code);
            }
        }
    }

    /// Push a message to a single player. Returns true if delivered
    /// (sender exists and channel still open), false otherwise.
    pub async fn send_to(&self, code: &str, player_id: &str, msg: ServerMsg) -> bool {
        let rooms = self.rooms.read().await;
        rooms
            .get(code)
            .and_then(|r| r.senders.get(player_id))
            .map(|tx| tx.send(msg).is_ok())
            .unwrap_or(false)
    }

    /// Broadcast a message to every player in the room. If `except` is
    /// `Some(player_id)`, that sender is skipped (used so a peer doesn't
    /// receive an echo of their own announcement).
    pub async fn broadcast(&self, code: &str, msg: ServerMsg, except: Option<&str>) {
        let rooms = self.rooms.read().await;
        if let Some(room) = rooms.get(code) {
            for (pid, tx) in &room.senders {
                if Some(pid.as_str()) == except {
                    continue;
                }
                let _ = tx.send(msg.clone());
            }
        }
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
        let _rx = hub.join_room("ABC", "0000001").await;
        let members = hub.room_members("ABC").await;
        assert_eq!(members, vec!["0000001".to_string()]);
    }

    #[tokio::test]
    async fn send_to_delivers_only_to_the_named_player() {
        let hub = SignalHub::default();
        let mut rx_a = hub.join_room("ABC", "0000001").await;
        let mut rx_b = hub.join_room("ABC", "0000002").await;

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
        let _rx = hub.join_room("ABC", "0000001").await;
        let delivered = hub
            .send_to("ABC", "0000099", ServerMsg::Kicked { reason: "test" })
            .await;
        assert!(!delivered);
    }

    #[tokio::test]
    async fn broadcast_reaches_everyone_when_except_is_none() {
        let hub = SignalHub::default();
        let mut rx_a = hub.join_room("ABC", "0000001").await;
        let mut rx_b = hub.join_room("ABC", "0000002").await;

        hub.broadcast("ABC", ServerMsg::PeerJoined { player_id: "X".into() }, None)
            .await;

        assert!(matches!(rx_a.try_recv(), Ok(ServerMsg::PeerJoined { .. })));
        assert!(matches!(rx_b.try_recv(), Ok(ServerMsg::PeerJoined { .. })));
    }

    #[tokio::test]
    async fn broadcast_skips_the_excepted_player() {
        let hub = SignalHub::default();
        let mut rx_a = hub.join_room("ABC", "0000001").await;
        let mut rx_b = hub.join_room("ABC", "0000002").await;

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
        {
            let _rx = hub.join_room("ABC", "0000001").await;
        }
        hub.leave_room("ABC", "0000001").await;
        assert!(hub.room_members("ABC").await.is_empty());
        // Internal: the rooms map should no longer contain "ABC" — verify
        // indirectly by attempting a send_to and confirming false.
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
                let _rx = hub.join_room("ABC", "0000001").await;
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            })
        };
        let h2 = {
            let hub = hub.clone();
            tokio::spawn(async move {
                let _rx = hub.join_room("ABC", "0000002").await;
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            })
        };
        h1.await.unwrap();
        h2.await.unwrap();

        let mut members = hub.room_members("ABC").await;
        members.sort();
        assert_eq!(members, vec!["0000001".to_string(), "0000002".to_string()]);
    }
}
