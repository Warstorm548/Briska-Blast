//! Client-IP pseudonymization + log-line redaction.
//!
//! Two guarantees, both so a raw client IP never reaches an operator's eyes:
//!   1. [`hash_ip_token`] turns an IP into a stable, non-reversible token, so
//!      the server's *own* logs can point at "the same client" (rate-limit
//!      abuse, repeated failures) without storing a raw address.
//!   2. [`redact_ips`] scrubs every IPv4/IPv6 literal out of an arbitrary log
//!      line. This is the render-boundary control for the admin Logs tab, which
//!      shows container output we don't author (redis, watchtower, deps): if
//!      every line passes through here first, no raw IP can reach the browser
//!      or a downloaded `.log`.
//!
//! The token is `HMAC-SHA256(key, SALT || ip)` truncated to 12 hex chars,
//! rendered `⟨ip:xxxxxxxxxxxx⟩`. Deterministic (identical IPs collapse to one
//! token, so patterns stay visible) but non-reversible: the IPv4 space is small
//! enough to brute-force a bare hash, so the secret `key` — a per-deployment
//! pepper, or a per-boot random key when none is configured (see
//! `state::AppState`) — is what actually protects it. The IP is normalized
//! through `IpAddr` before hashing, so `hash_ip_token` (source side) and
//! `redact_ips` (render side) yield the *same* token for the same address.

use std::net::IpAddr;
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use regex::Regex;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Domain-separation salt mixed in before the IP. Fixed (not per-record) so the
/// token stays deterministic for correlation; the secret key is what provides
/// non-reversibility. Versioned so a future scheme change is unambiguous.
const SALT: &[u8] = b"briska-ip-redact-v1:";

/// How many hex chars of the HMAC to keep. 12 (48 bits) is ample separation for
/// human log-reading while staying compact.
const TOKEN_HEX_LEN: usize = 12;

/// Pseudonymize one IP into `ip:<12 hex>` (no wrapping brackets — callers that
/// log structured fields want the bare token; [`redact_ips`] adds the brackets
/// for in-line display). `ip` is hashed verbatim; pass a canonical form
/// (`IpAddr::to_string`) for cross-call consistency.
pub fn hash_ip_token(key: &[u8], ip: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts a key of any length");
    mac.update(SALT);
    mac.update(ip.as_bytes());
    let mut hex = hex::encode(mac.finalize().into_bytes());
    hex.truncate(TOKEN_HEX_LEN);
    format!("ip:{hex}")
}

/// The bracketed display form used in redacted log lines: `⟨ip:xxxx⟩`.
fn redacted(key: &[u8], ip: &IpAddr) -> String {
    format!("\u{27e8}{}\u{27e9}", hash_ip_token(key, &ip.to_string()))
}

/// IPv4 dotted-quad candidate. Over-matching (e.g. an invalid `999.1.1.1`) is
/// harmless — the replace closure only rewrites candidates that actually parse
/// as an `IpAddr`, so version strings like `0.27.0` (three parts) and invalid
/// quads are left untouched.
fn ipv4_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{1,3}(?:\.\d{1,3}){3}\b").expect("valid IPv4 regex"))
}

/// IPv6 candidate — the widely-used comprehensive pattern (full, compressed
/// `::`, and IPv4-mapped tails). Every match is still `IpAddr`-validated before
/// redaction, so false positives (e.g. the `12:03:11` of a timestamp, which is
/// not a valid IPv6) are ignored.
fn ipv6_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Comprehensive IPv6 alternation (full 8-group, every `::` compression
        // position). Joined from parts for readability; each match is still
        // IpAddr-validated before redaction, so an over-match can't misfire.
        let alts = [
            r"(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}",
            r"(?:[0-9a-f]{1,4}:){1,7}:",
            r"(?:[0-9a-f]{1,4}:){1,6}:[0-9a-f]{1,4}",
            r"(?:[0-9a-f]{1,4}:){1,5}(?::[0-9a-f]{1,4}){1,2}",
            r"(?:[0-9a-f]{1,4}:){1,4}(?::[0-9a-f]{1,4}){1,3}",
            r"(?:[0-9a-f]{1,4}:){1,3}(?::[0-9a-f]{1,4}){1,4}",
            r"(?:[0-9a-f]{1,4}:){1,2}(?::[0-9a-f]{1,4}){1,5}",
            r"[0-9a-f]{1,4}:(?::[0-9a-f]{1,4}){1,6}",
            r":(?:(?::[0-9a-f]{1,4}){1,7}|:)",
        ];
        Regex::new(&format!("(?i){}", alts.join("|"))).expect("valid IPv6 regex")
    })
}

/// Rewrite every IPv4/IPv6 literal in `line` to a `⟨ip:…⟩` token. IPv4 first,
/// then IPv6 (tokens contain no colon-separated hex groups, so the second pass
/// can't re-match the first pass's output). Any candidate that doesn't parse as
/// a real IP is left exactly as it was.
pub fn redact_ips(key: &[u8], line: &str) -> String {
    let sub = |re: &Regex, s: &str| -> String {
        re.replace_all(s, |caps: &regex::Captures| {
            let m = &caps[0];
            match m.parse::<IpAddr>() {
                Ok(ip) => redacted(key, &ip),
                Err(_) => m.to_string(),
            }
        })
        .into_owned()
    };
    let after_v4 = sub(ipv4_re(), line);
    sub(ipv6_re(), &after_v4)
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: &[u8] = b"test-pepper";

    #[test]
    fn token_is_deterministic_and_key_sensitive() {
        assert_eq!(hash_ip_token(K, "1.2.3.4"), hash_ip_token(K, "1.2.3.4"));
        assert_ne!(hash_ip_token(K, "1.2.3.4"), hash_ip_token(K, "1.2.3.5"));
        assert_ne!(
            hash_ip_token(K, "1.2.3.4"),
            hash_ip_token(b"other-pepper", "1.2.3.4")
        );
        assert!(hash_ip_token(K, "1.2.3.4").starts_with("ip:"));
        assert_eq!(hash_ip_token(K, "1.2.3.4").len(), 3 + TOKEN_HEX_LEN);
    }

    #[test]
    fn redacts_single_ipv4() {
        let out = redact_ips(K, "client 192.168.1.55 connected");
        assert!(!out.contains("192.168.1.55"), "raw IP leaked: {out}");
        assert!(out.contains("\u{27e8}ip:"), "no token: {out}");
    }

    #[test]
    fn redacts_multiple_ipv4_in_one_line_and_keeps_port() {
        let out = redact_ips(K, "from 10.0.0.1:6379 to 10.0.0.2:5432");
        assert!(!out.contains("10.0.0.1"));
        assert!(!out.contains("10.0.0.2"));
        // Port is not PII and is left visible for debugging.
        assert!(out.contains(":6379"), "port dropped: {out}");
        assert!(out.contains(":5432"));
    }

    #[test]
    fn identical_ip_collapses_to_same_token() {
        let out = redact_ips(K, "a 8.8.8.8 b 8.8.8.8");
        let tok = hash_ip_token(K, "8.8.8.8");
        assert_eq!(out.matches(&tok).count(), 2, "not correlated: {out}");
    }

    #[test]
    fn redacts_ipv6_compressed_and_full() {
        let out = redact_ips(K, "peer [::1]:8080 and 2001:db8::1 done");
        assert!(!out.contains("::1]"), "compressed v6 leaked: {out}");
        assert!(!out.contains("2001:db8::1"), "full v6 leaked: {out}");
    }

    #[test]
    fn leaves_non_ip_text_untouched() {
        // Version string (3 parts) and a timestamp must survive verbatim.
        let line = "server v0.27.0 started at 2026-07-21 12:03:11 INFO";
        assert_eq!(redact_ips(K, line), line);
    }
}
