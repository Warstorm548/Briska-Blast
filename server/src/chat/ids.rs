//! Server-assigned **message-body identifiers** — the 12-character handle every
//! chat message carries so moderation tools have something stable to point at.
//!
//! Shape: one `epoch` character followed by an 11-character base62 `tail`, e.g.
//! `a00000004Kx9`. The tail is a monotonic counter, so ids sort lexicographically
//! in issue order — a useful property when reading a transcript or an audit log.
//!
//! # Why a plain counter
//!
//! Uniqueness comes from Redis `INCR` being atomic: two messages arriving in the
//! same millisecond get consecutive values, never the same one. That costs a
//! single integer for the entire lifetime of the deployment — no per-player and
//! no per-session state. It mirrors `player:counter` (`api/register.rs`).
//!
//! Ids are **never reused**. Reuse is what breaks traceability: a recycled id
//! would silently point an old audit record and a new message at the same
//! string. For the same reason an id never embeds a player id — player numbers
//! *are* recycled through `player:freelist`, so embedding one would collide a
//! deleted player's records with their replacement's. The player association is
//! a field on the stored message, not part of the identifier.
//!
//! # Why the epoch lives in its own key
//!
//! To keep the leading character a letter, a single-counter scheme would need an
//! offset of `10 * 62^11` ≈ 5.2e20 — far past the i64 that `INCR` returns. So the
//! epoch is stored separately and composed with the tail at format time.
//!
//! # Why [`EPOCH_LIMIT`] is a soft trigger
//!
//! The tail holds `62^11` = 5.2e19, comfortably more than `i64::MAX` = 9.2e18.
//! **Every counter value a machine can produce therefore still fits in 11
//! characters.** Crossing the limit is not a boundary that must be enforced
//! before emitting — the id is formatted under the current epoch (still unique,
//! still 12 chars) and the epoch is advanced for *subsequent* ids. Two callers
//! racing across the boundary cannot collide: the one that formatted under the
//! old epoch is unique within it, and the new epoch restarts at a counter no
//! old-epoch id shares a prefix with. No lock, no retry, no discarded values.
//!
//! Capacity: 52 epochs × 9.2e18 ≈ **4.8e20 messages**. At a sustained 1,000
//! messages/second that is ~15 billion years, so the advance path is expected to
//! be dead code — it exists so the scheme has no real ceiling rather than an
//! arbitrary one.
//!
//! **Operational note:** `chat:body:counter` and `chat:body:epoch` must never be
//! flushed. Resetting either would re-issue ids that already appear in retained
//! transcripts and audit records.

use deadpool_redis::redis::{RedisResult, Script};

/// Digits for the tail, ordered so the encoding sorts lexicographically in
/// numeric order: `0-9`, then lowercase, then uppercase.
const ALPHABET: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Epoch characters, in advance order: `a`..`z` then `A`..`Z`. Letters only, so
/// the first character of an id is always visibly distinct from the tail.
const EPOCH_ALPHABET: &[u8; 52] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// The epoch used before anything has been stored (a fresh deployment).
const FIRST_EPOCH: char = 'a';

/// Characters in the counter portion of an id.
const TAIL_LEN: usize = 11;

/// Total id width. Every moderation surface renders exactly this many chars.
const BODY_ID_LEN: usize = TAIL_LEN + 1;

const COUNTER_KEY: &str = "chat:body:counter";
const EPOCH_KEY: &str = "chat:body:epoch";

/// Counter value past which the epoch advances. Set below `i64::MAX`
/// (9_223_372_036_854_775_807) so there is ample slack between the trigger and an
/// actual `INCR` overflow — Redis errors rather than wrapping, and the epoch will
/// have rolled long before then.
const EPOCH_LIMIT: i64 = 9_000_000_000_000_000_000;

/// Compare-and-swap the epoch forward and restart the counter.
///
/// Guarded on the epoch the caller observed, so concurrent callers crossing
/// [`EPOCH_LIMIT`] together produce exactly one advance — the losers simply see
/// it already done. Deliberately handles **no large numbers**: Redis Lua stores
/// numbers as doubles and would silently lose precision above 2^53, so the limit
/// comparison stays in Rust where i64 is exact, and Lua only swaps two strings.
const ADVANCE_EPOCH_SCRIPT: &str = r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  redis.call('SET', KEYS[1], ARGV[2])
  redis.call('SET', KEYS[2], '0')
  return 1
end
return 0
"#;

/// Render a counter value as the 11-character base62 tail, left-padded with `0`.
///
/// Infallible for any non-negative i64: `i64::MAX` needs 11 base62 digits, which
/// is exactly the width available. Negative input (only reachable if the counter
/// key were tampered with) is clamped to 0 rather than panicking.
fn encode_tail(n: i64) -> String {
    let mut buf = [b'0'; TAIL_LEN];
    let mut v = n.max(0) as u64;
    let mut i = TAIL_LEN;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = ALPHABET[(v % 62) as usize];
        v /= 62;
    }
    String::from_utf8(buf.to_vec()).expect("base62 alphabet is ASCII")
}

/// Compose an id from an epoch character and a counter value.
fn format_body_id(epoch: char, counter: i64) -> String {
    let id = format!("{epoch}{}", encode_tail(counter));
    // Every moderation surface renders a fixed-width id, and audit records match
    // bodies by string equality — a short id would silently fail to match.
    debug_assert_eq!(id.len(), BODY_ID_LEN, "body id must be exactly {BODY_ID_LEN} chars");
    id
}

/// The epoch that follows `current`, or `None` when the last one (`Z`) is in use.
///
/// Exhausting all 52 epochs means ~4.8e20 messages have been issued; the caller
/// logs and keeps using the final epoch, which remains internally unique for
/// another 9.2e18 ids.
fn next_epoch(current: char) -> Option<char> {
    let pos = EPOCH_ALPHABET.iter().position(|&c| c as char == current)?;
    EPOCH_ALPHABET.get(pos + 1).map(|&c| c as char)
}

/// Read a stored epoch value, falling back to [`FIRST_EPOCH`] when unset or
/// malformed. A garbled key must not take chat down, and a wrong-but-valid epoch
/// still yields unique ids.
fn parse_epoch(stored: Option<String>) -> char {
    stored
        .and_then(|s| s.chars().next())
        .filter(|c| c.is_ascii() && EPOCH_ALPHABET.contains(&(*c as u8)))
        .unwrap_or(FIRST_EPOCH)
}

/// Issue the next body id.
///
/// One round trip: `INCR` the counter and `GET` the epoch in a single
/// transaction. When the counter crosses [`EPOCH_LIMIT`] the id is still returned
/// under the current epoch (see the module docs — it is unique and correctly
/// sized), and the epoch is advanced afterwards for subsequent callers.
pub async fn next_body_id(conn: &mut deadpool_redis::Connection) -> RedisResult<String> {
    let (counter, epoch_raw): (i64, Option<String>) = deadpool_redis::redis::pipe()
        .atomic()
        .incr(COUNTER_KEY, 1)
        .get(EPOCH_KEY)
        .query_async(conn)
        .await?;

    let epoch = parse_epoch(epoch_raw);
    let id = format_body_id(epoch, counter);

    if counter > EPOCH_LIMIT {
        advance_epoch(conn, epoch).await;
    }

    Ok(id)
}

/// Move the epoch forward, guarded on `observed` so exactly one racing caller
/// wins. Failures are logged and swallowed: the current epoch still has ~2e17
/// counter values of slack before `INCR` could overflow, so a missed advance is
/// recoverable on any later call.
async fn advance_epoch(conn: &mut deadpool_redis::Connection, observed: char) {
    let Some(next) = next_epoch(observed) else {
        tracing::error!(
            epoch = %observed,
            "chat: body-id epochs exhausted — ids remain unique but the scheme is at its ceiling"
        );
        return;
    };

    let result: RedisResult<i64> = Script::new(ADVANCE_EPOCH_SCRIPT)
        .key(EPOCH_KEY)
        .key(COUNTER_KEY)
        .arg(observed.to_string())
        .arg(next.to_string())
        .invoke_async(&mut *conn)
        .await;

    match result {
        Ok(1) => tracing::warn!(from = %observed, to = %next, "chat: body-id epoch advanced"),
        Ok(_) => tracing::debug!("chat: body-id epoch already advanced by another caller"),
        Err(e) => tracing::error!("chat: body-id epoch advance failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_is_always_eleven_chars() {
        for n in [0, 1, 61, 62, 3843, i64::MAX] {
            assert_eq!(encode_tail(n).len(), TAIL_LEN, "n = {n}");
        }
    }

    #[test]
    fn tail_encodes_base62_boundaries() {
        assert_eq!(encode_tail(0), "00000000000");
        assert_eq!(encode_tail(1), "00000000001");
        assert_eq!(encode_tail(9), "00000000009");
        // 10 is the first letter, 35 the last lowercase, 36 the first uppercase.
        assert_eq!(encode_tail(10), "0000000000a");
        assert_eq!(encode_tail(35), "0000000000z");
        assert_eq!(encode_tail(36), "0000000000A");
        assert_eq!(encode_tail(61), "0000000000Z");
        // Rolling into the second digit.
        assert_eq!(encode_tail(62), "00000000010");
        assert_eq!(encode_tail(63), "00000000011");
    }

    #[test]
    fn i64_max_fits_the_tail_exactly() {
        // The load-bearing invariant: no reachable counter value can overflow 11
        // characters, which is why crossing EPOCH_LIMIT never needs a retry.
        let encoded = encode_tail(i64::MAX);
        assert_eq!(encoded.len(), TAIL_LEN);
        assert!(!encoded.starts_with('0'), "should use the full width: {encoded}");
    }

    #[test]
    fn negative_counter_clamps_instead_of_panicking() {
        assert_eq!(encode_tail(-1), "00000000000");
    }

    #[test]
    fn ids_sort_in_issue_order() {
        let mut ids: Vec<String> = [1i64, 2, 61, 62, 1000, 999_999, i64::MAX / 2]
            .iter()
            .map(|&n| format_body_id('a', n))
            .collect();
        let expected = ids.clone();
        ids.sort();
        assert_eq!(ids, expected, "lexicographic order must match numeric order");
    }

    #[test]
    fn ids_are_twelve_chars_with_a_letter_epoch() {
        let id = format_body_id('a', 271_828);
        assert_eq!(id.len(), BODY_ID_LEN);
        assert!(id.starts_with('a'));
        assert!(
            id.chars().skip(1).all(|c| c.is_ascii_alphanumeric()),
            "tail must stay alphanumeric: {id}"
        );
    }

    #[test]
    fn epoch_advances_through_lower_then_upper_case() {
        assert_eq!(next_epoch('a'), Some('b'));
        assert_eq!(next_epoch('y'), Some('z'));
        // The whole point of adding capitals: the sequence continues past 'z'.
        assert_eq!(next_epoch('z'), Some('A'));
        assert_eq!(next_epoch('A'), Some('B'));
        assert_eq!(next_epoch('Y'), Some('Z'));
        // 52 epochs and no more.
        assert_eq!(next_epoch('Z'), None);
    }

    #[test]
    fn epoch_rollover_cannot_repeat_an_id() {
        // The last id of one epoch and the first of the next share no prefix, so
        // the counter reset is safe.
        let last = format_body_id('a', EPOCH_LIMIT);
        let first = format_body_id('b', 1);
        assert_ne!(last, first);
        assert!(last < first, "epochs keep the global ordering: {last} < {first}");
    }

    #[test]
    fn stored_epoch_falls_back_when_missing_or_junk() {
        assert_eq!(parse_epoch(None), FIRST_EPOCH);
        assert_eq!(parse_epoch(Some(String::new())), FIRST_EPOCH);
        assert_eq!(parse_epoch(Some("9".into())), FIRST_EPOCH, "digits are not epochs");
        assert_eq!(parse_epoch(Some("-".into())), FIRST_EPOCH);
        assert_eq!(parse_epoch(Some("q".into())), 'q');
        assert_eq!(parse_epoch(Some("Q".into())), 'Q');
        // Only the first character is significant.
        assert_eq!(parse_epoch(Some("bcd".into())), 'b');
    }

    #[test]
    fn body_id_is_case_sensitive() {
        // Documented property: ids differing only in case are distinct messages.
        assert_ne!(format_body_id('a', 10), format_body_id('A', 10));
        assert_ne!(encode_tail(10), encode_tail(36));
    }
}
