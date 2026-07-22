//! Client-IP redaction for the admin Logs **web render boundary**.
//!
//! [`redact_ips`] rewrites every IPv4/IPv6 literal in a log line to a stable,
//! non-reversible `⟨ip:…⟩` token. The guarantee it provides is scoped precisely:
//! **no raw client IP reaches the browser or a downloaded `.log`**, because the
//! Logs handler runs every line through it before the bytes leave the server.
//!
//! This is deliberately *not* a claim that raw IPs never reach operators or the
//! host's logs. The server logs raw client IPs on abuse events (`admin/auth.rs`
//! and the API rate-limit paths) precisely so an operator with direct host
//! access (`docker logs` / SSH) can trace a bad actor. Those raw IPs live in the
//! host's container logs; redaction happens only when they are rendered to the
//! web.
//!
//! [`hash_ip_token`] is the token primitive: `HMAC-SHA256(key, SALT || ip)`
//! truncated to 12 hex chars, rendered `ip:xxxxxxxxxxxx` (the brackets are added
//! by [`redact_ips`]). Deterministic (identical IPs collapse to one token, so
//! patterns stay visible) but non-reversible: the IPv4 space is small enough to
//! brute-force a bare hash, so the secret `key` — a per-deployment pepper, or a
//! per-boot random key when none is configured (see `state::AppState`) — is what
//! protects it. The IP is normalized through `IpAddr` before hashing.

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

/// IPv6 candidate: a maximal run of hex/dot characters containing at least two
/// colon-separated groups. Because it grabs the *whole* run, it can't truncate a
/// compressed address (`2001:db8::1`) or split an IPv4-mapped one
/// (`::ffff:192.0.2.1`) — and requiring ≥2 colons means a bare `host:port` (one
/// colon) is left for the IPv4 pass. Every match is `Ipv6Addr`-validated before
/// replacement, so a timestamp like `12:03:11` is ignored.
fn ipv6_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[0-9a-fA-F.]*(?::[0-9a-fA-F.]*){2,}").expect("valid IPv6 regex")
    })
}

/// Rewrite every IPv4/IPv6 literal in `line` to a `⟨ip:…⟩` token. **IPv6 runs
/// first** so an IPv4-mapped address is taken as one token rather than split by
/// the IPv4 pass; the tokens it emits contain no dotted quad, so the IPv4 pass
/// can't re-match them. Any candidate that doesn't parse as a real IP is left
/// exactly as it was.
pub fn redact_ips(key: &[u8], line: &str) -> String {
    let after_v6 = ipv6_re()
        .replace_all(line, |caps: &regex::Captures| {
            let m = &caps[0];
            // A candidate can pick up a trailing '.' (e.g. "::1." at a sentence
            // end); a trailing "::" is a legal all-zeros suffix, so only '.' is
            // trimmed. Redact the address and re-append any trimmed punctuation.
            let addr = m.trim_end_matches('.');
            match addr.parse::<std::net::Ipv6Addr>() {
                Ok(v6) => format!("{}{}", redacted(key, &IpAddr::V6(v6)), &m[addr.len()..]),
                Err(_) => m.to_string(),
            }
        })
        .into_owned();
    ipv4_re()
        .replace_all(&after_v6, |caps: &regex::Captures| {
            match caps[0].parse::<std::net::Ipv4Addr>() {
                Ok(v4) => redacted(key, &IpAddr::V4(v4)),
                Err(_) => caps[0].to_string(),
            }
        })
        .into_owned()
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

    /// Exact output: a compressed IPv6 is replaced whole, not truncated to
    /// `2001:db8::` with a stray `1` left behind.
    #[test]
    fn ipv6_compressed_is_one_untruncated_token() {
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        let tok = format!("\u{27e8}{}\u{27e9}", hash_ip_token(K, &ip.to_string()));
        assert_eq!(redact_ips(K, "peer 2001:db8::1 done"), format!("peer {tok} done"));
    }

    /// Exact output: an IPv4-mapped IPv6 literal is one token, not split into a
    /// v6 head and a separately-redacted v4 tail by the IPv4 pass.
    #[test]
    fn ipv4_mapped_ipv6_is_single_token() {
        let ip: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
        let tok = format!("\u{27e8}{}\u{27e9}", hash_ip_token(K, &ip.to_string()));
        assert_eq!(redact_ips(K, "from ::ffff:192.0.2.1 ok"), format!("from {tok} ok"));
    }

    #[test]
    fn leaves_non_ip_text_untouched() {
        // Version string (3 parts) and a timestamp must survive verbatim.
        let line = "server v0.27.0 started at 2026-07-21 12:03:11 INFO";
        assert_eq!(redact_ips(K, line), line);
    }
}
