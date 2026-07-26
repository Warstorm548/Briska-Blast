//! The word blacklist: storage, matching, and censoring.
//!
//! A blacklisted word does two things when it appears in a message. On the game
//! side it is **masked before broadcast**, so players receive `##### you all`
//! rather than the original — censoring therefore needs no protocol change and no
//! client change, because the server never sends the raw word. On the moderation
//! side the *original* text is kept in the transcript along with the exact match
//! offsets, so the panel can highlight what fired and a moderator can judge it in
//! context.
//!
//! # Matching rules
//!
//! Case-insensitive, whole-word only: `scrub` fires on `Scrub` and `SCRUB` but
//! never inside `scrubbing`, matching the boundary rule the admin templates
//! already render with (`templates/chatmod.rs::highlight_with`).
//!
//! Case-insensitivity is **ASCII-only, deliberately**. The obvious implementation
//! — lowercase the text, then search — is wrong here: Unicode lowercasing can
//! change a string's byte length (`İ` U+0130 becomes two chars), which would slide
//! every offset after it and corrupt both the mask and the highlight. Comparing
//! bytes with `eq_ignore_ascii_case` keeps offsets exact, and is safe on
//! multi-byte input because no continuation byte can equal an ASCII byte under
//! case folding. A non-ASCII blacklist word still matches, just case-sensitively.
//!
//! Overlapping matches are resolved rather than left ambiguous (earliest start
//! wins, longest breaks the tie), so [`mask`] has exactly one interpretation.
//!
//! # Caching
//!
//! Matching runs on every accepted chat message, so the word list is held in
//! memory rather than fetched per message. [`invalidate`] is called by the admin
//! write paths; [`CACHE_TTL`] is the backstop that converges anyway if a write
//! happens somewhere unexpected (a manual `redis-cli` edit, say).

use std::sync::Arc;
use std::time::{Duration, Instant};

use deadpool_redis::redis::{AsyncCommands, RedisResult};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Redis hash of blacklisted words. Field = the normalized (lowercased) word,
/// value = a JSON [`BlacklistEntry`]. No TTL — see the module docs in [`super`].
const BLACKLIST_KEY: &str = "chat:blacklist";

/// How long a cached word list is trusted without re-reading Redis. Short enough
/// that an out-of-band edit converges quickly, long enough that the chat path
/// almost never pays for a round trip.
const CACHE_TTL: Duration = Duration::from_secs(30);

/// A word on the blacklist, as stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
    /// The word as the moderator typed it — used for display in the lists table.
    /// The Redis field key is the lowercased form; this preserves their casing.
    pub word: String,
    /// Why it was added. Logging-only: never sent to a player (0.30.0 contract).
    pub reason: String,
    /// The "Active Filter Toggle" column. An inactive word stays on the list and
    /// in the audit history but stops matching.
    #[serde(default = "default_true")]
    pub active: bool,
    /// Display name of the moderator who added it.
    #[serde(default)]
    pub added_by: String,
    #[serde(default)]
    pub added_at_ms: i64,
}

fn default_true() -> bool {
    true
}

/// One occurrence of a blacklisted word in a message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Byte offset into the original text.
    pub start: usize,
    /// Byte length of the matched span. Equal to the word's byte length.
    pub len: usize,
    /// The normalized blacklist word that fired.
    pub word: String,
}

impl Match {
    fn end(&self) -> usize {
        self.start + self.len
    }
}

/// The in-memory word list. Holds only *active* words, already normalized —
/// the matching path should never have to filter or lowercase.
#[derive(Default)]
pub struct BlacklistCache {
    words: Arc<Vec<String>>,
    loaded_at: Option<Instant>,
}

impl BlacklistCache {
    fn is_fresh(&self) -> bool {
        self.loaded_at
            .is_some_and(|t| t.elapsed() < CACHE_TTL)
    }
}

/// Normalized storage key for a word: trimmed and ASCII-lowercased, so `Scrub`
/// and `scrub` can never both occupy the list.
pub fn normalize(word: &str) -> String {
    word.trim().to_ascii_lowercase()
}

/// Read the whole blacklist, newest-first by insertion time.
pub async fn load(conn: &mut deadpool_redis::Connection) -> RedisResult<Vec<BlacklistEntry>> {
    let raw: std::collections::HashMap<String, String> = conn.hgetall(BLACKLIST_KEY).await?;
    let mut entries: Vec<BlacklistEntry> = raw
        .into_iter()
        // A single corrupt field must not blank the whole moderation list.
        .filter_map(|(key, json)| match serde_json::from_str::<BlacklistEntry>(&json) {
            Ok(entry) => Some(entry),
            Err(e) => {
                tracing::warn!(word = %key, "chat: skipping malformed blacklist entry: {}", e);
                None
            }
        })
        .collect();
    entries.sort_by(|a, b| b.added_at_ms.cmp(&a.added_at_ms).then(a.word.cmp(&b.word)));
    Ok(entries)
}

/// The active word list, from cache when fresh.
///
/// Returns an `Arc` so the per-message path clones a pointer, not the list. A
/// Redis failure serves the previous snapshot rather than silently disabling the
/// filter — stale matching is a far better failure mode than none.
pub async fn active_words(state: &AppState) -> Arc<Vec<String>> {
    {
        let cache = state.chat_blacklist.read().await;
        if cache.is_fresh() {
            return Arc::clone(&cache.words);
        }
    }

    let loaded = match state.redis.get().await {
        Ok(mut conn) => load(&mut conn).await,
        Err(e) => Err(deadpool_redis::redis::RedisError::from((
            deadpool_redis::redis::ErrorKind::IoError,
            "pool",
            e.to_string(),
        ))),
    };

    match loaded {
        Ok(entries) => {
            let words: Vec<String> = entries
                .iter()
                .filter(|e| e.active)
                .map(|e| normalize(&e.word))
                .filter(|w| !w.is_empty())
                .collect();
            let words = Arc::new(words);
            let mut cache = state.chat_blacklist.write().await;
            cache.words = Arc::clone(&words);
            cache.loaded_at = Some(Instant::now());
            words
        }
        Err(e) => {
            tracing::warn!("chat: blacklist reload failed, serving cached list: {}", e);
            Arc::clone(&state.chat_blacklist.read().await.words)
        }
    }
}

/// Force the next [`active_words`] call to re-read Redis. Called by the admin
/// add/remove/toggle handlers so an edit takes effect immediately.
pub async fn invalidate(state: &AppState) {
    state.chat_blacklist.write().await.loaded_at = None;
}

/// What an [`add`] call actually did, split so the caller can report the
/// difference rather than silently swallowing it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AddOutcome {
    /// Words that were newly written.
    pub added: Vec<String>,
    /// Words already on the list, left untouched.
    pub duplicates: Vec<String>,
}

/// Add words to the blacklist.
///
/// Duplicates are impossible by construction: the field key is the normalized
/// word, so `Frick`, `frick` and `  FRICK  ` are one entry, and `HSETNX` means an
/// existing word is never overwritten. That matters beyond tidiness — the first
/// add already wrote an audit record naming its author and reason, and silently
/// rewriting the entry would leave the list disagreeing with the log.
///
/// Already-present words come back in [`AddOutcome::duplicates`] so the caller can
/// say which ones were skipped instead of just reporting a smaller count.
pub async fn add(
    conn: &mut deadpool_redis::Connection,
    words: &[String],
    reason: &str,
    added_by: &str,
) -> RedisResult<AddOutcome> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut out = AddOutcome::default();
    for raw in words {
        let key = normalize(raw);
        if key.is_empty() {
            continue;
        }
        // A word repeated within one submission is a duplicate of itself; report
        // it once rather than counting it twice.
        if out.added.contains(&key) || out.duplicates.contains(&key) {
            continue;
        }
        let entry = BlacklistEntry {
            word: raw.trim().to_string(),
            reason: reason.to_string(),
            active: true,
            added_by: added_by.to_string(),
            added_at_ms: now,
        };
        let json = serde_json::to_string(&entry).unwrap_or_default();
        let inserted: bool = conn.hset_nx(BLACKLIST_KEY, &key, json).await?;
        if inserted {
            out.added.push(key);
        } else {
            out.duplicates.push(key);
        }
    }
    Ok(out)
}

/// Remove words. Returns those that were actually on the list.
pub async fn remove(
    conn: &mut deadpool_redis::Connection,
    words: &[String],
) -> RedisResult<Vec<String>> {
    let mut removed = Vec::new();
    for raw in words {
        let key = normalize(raw);
        if key.is_empty() {
            continue;
        }
        let deleted: i64 = conn.hdel(BLACKLIST_KEY, &key).await?;
        if deleted > 0 {
            removed.push(key);
        }
    }
    Ok(removed)
}

/// Flip a word's active filter. Returns false when the word isn't on the list.
pub async fn set_active(
    conn: &mut deadpool_redis::Connection,
    word: &str,
    active: bool,
) -> RedisResult<bool> {
    let key = normalize(word);
    let Some(json): Option<String> = conn.hget(BLACKLIST_KEY, &key).await? else {
        return Ok(false);
    };
    let Ok(mut entry) = serde_json::from_str::<BlacklistEntry>(&json) else {
        return Ok(false);
    };
    entry.active = active;
    let updated = serde_json::to_string(&entry).unwrap_or(json);
    let _: () = conn.hset(BLACKLIST_KEY, &key, updated).await?;
    Ok(true)
}

/// Whether the character before `idx` allows a word to start there.
fn boundary_before(text: &str, idx: usize) -> bool {
    text[..idx].chars().next_back().is_none_or(|c| !c.is_alphanumeric())
}

/// Whether the character at `idx` allows a word to end there.
fn boundary_after(text: &str, idx: usize) -> bool {
    text[idx..].chars().next().is_none_or(|c| !c.is_alphanumeric())
}

/// Every blacklisted occurrence in `text`, ordered by position and free of
/// overlaps. See the module docs for the matching rules.
pub fn find_matches(text: &str, words: &[String]) -> Vec<Match> {
    let bytes = text.as_bytes();
    let mut found: Vec<Match> = Vec::new();

    for word in words {
        let needle = word.as_bytes();
        let n = needle.len();
        if n == 0 || n > bytes.len() {
            continue;
        }
        let mut i = 0;
        while i + n <= bytes.len() {
            // Only consider positions that slice the string legally; a match can
            // never begin or end mid-character.
            if !text.is_char_boundary(i) || !text.is_char_boundary(i + n) {
                i += 1;
                continue;
            }
            if bytes[i..i + n].eq_ignore_ascii_case(needle)
                && boundary_before(text, i)
                && boundary_after(text, i + n)
            {
                found.push(Match { start: i, len: n, word: word.clone() });
                i += n;
            } else {
                i += 1;
            }
        }
    }

    // Earliest start wins; longest breaks the tie. Without this, two overlapping
    // blacklist words would give `mask` two valid answers.
    found.sort_by(|a, b| a.start.cmp(&b.start).then(b.len.cmp(&a.len)));
    let mut deduped: Vec<Match> = Vec::with_capacity(found.len());
    for m in found {
        if deduped.last().is_none_or(|prev| m.start >= prev.end()) {
            deduped.push(m);
        }
    }
    deduped
}

/// Replace every match with `#`, one per character.
///
/// The mask is sized in **characters, not bytes**, so a multi-byte word is not
/// replaced by a visually longer run of hashes.
pub fn mask(text: &str, matches: &[Match]) -> String {
    if matches.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for m in matches {
        if m.start < cursor {
            continue; // defensive: find_matches already removes overlaps
        }
        out.push_str(&text[cursor..m.start]);
        for _ in 0..text[m.start..m.end()].chars().count() {
            out.push('#');
        }
        cursor = m.end();
    }
    out.push_str(&text[cursor..]);
    out
}

/// The distinct words that fired, in first-occurrence order — what an audit
/// record stores and the flagged-messages panel lists.
pub fn matched_words(matches: &[Match]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for m in matches {
        if !seen.contains(&m.word) {
            seen.push(m.word.clone());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| normalize(w)).collect()
    }

    #[test]
    fn matches_are_case_insensitive() {
        let list = words(&["frick"]);
        for body in ["frick you", "FRICK you", "FrIcK you"] {
            let m = find_matches(body, &list);
            assert_eq!(m.len(), 1, "{body}");
            assert_eq!(m[0].start, 0);
            assert_eq!(m[0].len, 5);
        }
    }

    #[test]
    fn matches_respect_word_boundaries() {
        let list = words(&["ass"]);
        // The classic false positive this rule exists to prevent.
        assert!(find_matches("that was classic", &list).is_empty());
        assert!(find_matches("assorted candy", &list).is_empty());
        assert!(find_matches("passable", &list).is_empty());
        // Standalone, including punctuation-adjacent, still fires.
        assert_eq!(find_matches("you ass", &list).len(), 1);
        assert_eq!(find_matches("ass!", &list).len(), 1);
        assert_eq!(find_matches("(ass)", &list).len(), 1);
    }

    #[test]
    fn finds_every_occurrence_and_several_words() {
        let list = words(&["frick", "scrub"]);
        let m = find_matches("frick you scrub, frick off", &list);
        assert_eq!(m.len(), 3);
        // Ordered by position regardless of the order words appear in the list.
        assert_eq!(m[0].start, 0);
        assert_eq!(m[1].start, 10);
        assert_eq!(m[2].start, 17);
        assert_eq!(matched_words(&m), vec!["frick", "scrub"]);
    }

    #[test]
    fn overlapping_matches_are_resolved_longest_first() {
        let list = words(&["bad", "badword"]);
        let m = find_matches("badword here", &list);
        assert_eq!(m.len(), 1, "overlap must collapse to one match");
        assert_eq!(m[0].word, "badword", "the longer word wins the tie");
    }

    #[test]
    fn inactive_words_are_simply_absent_from_the_list() {
        // active_words() filters before matching, so the matcher itself only ever
        // sees active words — an empty list matches nothing.
        assert!(find_matches("frick you all", &[]).is_empty());
    }

    #[test]
    fn multi_byte_text_does_not_panic_or_mismatch() {
        let list = words(&["frick"]);
        // Emoji and accented characters around the match.
        let body = "héllo 🎮 frick 🎮 wörld";
        let m = find_matches(body, &list);
        assert_eq!(m.len(), 1);
        assert_eq!(&body[m[0].start..m[0].start + m[0].len], "frick");
        // And a body that is entirely multi-byte finds nothing, without slicing panics.
        assert!(find_matches("日本語のテキスト", &list).is_empty());
    }

    #[test]
    fn multi_byte_needle_matches_exactly() {
        let list = vec!["café".to_string()];
        let m = find_matches("the café is open", &list);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].len, "café".len(), "byte length, not char count");
    }

    #[test]
    fn mask_replaces_matches_and_leaves_the_rest() {
        let list = words(&["frick"]);
        let body = "frick you all";
        let m = find_matches(body, &list);
        assert_eq!(mask(body, &m), "##### you all");
    }

    #[test]
    fn mask_is_sized_in_characters_not_bytes() {
        let list = vec!["café".to_string()];
        let body = "the café is open";
        let m = find_matches(body, &list);
        // 4 characters, 5 bytes — the mask must show 4 hashes.
        assert_eq!(mask(body, &m), "the #### is open");
    }

    #[test]
    fn mask_handles_several_matches_and_no_matches() {
        let list = words(&["frick", "scrub"]);
        let body = "frick you scrub";
        assert_eq!(mask(body, &find_matches(body, &list)), "##### you #####");
        assert_eq!(mask("clean message", &[]), "clean message");
    }

    #[test]
    fn mask_preserves_surrounding_multi_byte_text() {
        let list = words(&["frick"]);
        let body = "héllo 🎮 frick 🎮 wörld";
        let masked = mask(body, &find_matches(body, &list));
        assert_eq!(masked, "héllo 🎮 ##### 🎮 wörld");
    }

    #[test]
    fn normalization_makes_case_variants_the_same_entry() {
        // The whole duplicate guarantee rests on this: the hash field key is the
        // normalized word, so these can never occupy separate rows.
        for variant in ["Frick", "FRICK", "  frick  ", "fRiCk"] {
            assert_eq!(normalize(variant), "frick", "variant: {variant:?}");
        }
    }

    #[test]
    fn add_outcome_separates_new_words_from_duplicates() {
        // The struct itself is the contract the handler reports from; the Redis
        // round-trip is covered by the end-to-end checks, not here.
        let outcome = AddOutcome {
            added: vec!["frick".into()],
            duplicates: vec!["chicken".into()],
        };
        assert_eq!(outcome.added.len(), 1);
        assert_eq!(outcome.duplicates, vec!["chicken"]);
        assert_eq!(AddOutcome::default(), AddOutcome { added: vec![], duplicates: vec![] });
    }

    #[test]
    fn normalize_trims_and_lowercases() {
        assert_eq!(normalize("  Frick "), "frick");
        assert_eq!(normalize("SCRUB"), "scrub");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   "), "");
    }

    #[test]
    fn entry_deserializes_without_the_optional_fields() {
        // Forward compatibility with any entry written before a field was added.
        let entry: BlacklistEntry =
            serde_json::from_str(r#"{"word":"frick","reason":"Slur"}"#).unwrap();
        assert_eq!(entry.word, "frick");
        assert!(entry.active, "active must default to true, not false");
        assert_eq!(entry.added_by, "");
        assert_eq!(entry.added_at_ms, 0);
    }
}
