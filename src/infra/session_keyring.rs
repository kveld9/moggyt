//! OS-keyring session storage and cookie input sanitization.
//!
//! Provides persistence for authenticated YouTube Music session cookies
//! using the platform credential store (Secret Service on Linux, Credential
//! Manager on Windows, Keychain on macOS).
//!
//! # Provider-absent fallback (SDD D-2 fragmentation)
//! On systems without a configured or running secret provider (e.g. headless
//! Linux servers, minimal containers, or environments lacking D-Bus Secret
//! Service), keyring operations will fail softly with a warning log, and
//! `load()` will return `None`.

use super::SessionStore;

/// Maximum byte length for a single keyring chunk.
///
/// Windows Credential Manager rejects blobs above CRED_MAX_CREDENTIAL_BLOB_SIZE
/// (2560 bytes) with RPC_S_STRING_TOO_LONG, and Google session cookie headers
/// routinely exceed 3 KB.
pub const KEYRING_CHUNK_BYTES: usize = 2000;

/// Upper bound on the number of chunks for a single session payload.
///
/// Bounds a stored payload at roughly 128 KiB and rejects corrupt headers.
pub const KEYRING_MAX_CHUNKS: usize = 64;

const CHUNK_HEADER_PREFIX: &str = "moggyt-chunked-v1:";

/// Formats the account name for a numbered chunk.
///
/// Index 0 preserves the canonical account name for backward compatibility,
/// while index n > 0 appends a 1-based numbered suffix (e.g. `account #2`).
pub fn chunk_account(account: &str, index: usize) -> String {
    if index == 0 { account.to_owned() } else { format!("{account} #{}", index + 1) }
}

fn split_at_char_boundary(payload: &str, max_bytes: usize) -> usize {
    if payload.len() <= max_bytes {
        return payload.len();
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !payload.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn split_payload_chunks(payload: &str) -> Vec<&str> {
    if payload.is_empty() {
        return vec![""];
    }
    let mut chunks = Vec::new();
    let mut remaining = payload;
    while !remaining.is_empty() {
        let split_idx = split_at_char_boundary(remaining, KEYRING_CHUNK_BYTES);
        let split_idx = if split_idx == 0 { remaining.len() } else { split_idx };
        let (chunk, rest) = remaining.split_at(split_idx);
        chunks.push(chunk);
        remaining = rest;
    }
    chunks
}

/// Splits a payload into numbered chunks suitable for keyring storage.
pub fn encode_entries(payload: &str) -> Vec<String> {
    let chunks = split_payload_chunks(payload);
    let count = chunks.len();
    let mut entries = Vec::with_capacity(count);
    if let Some((first, rest)) = chunks.split_first() {
        entries.push(format!("{CHUNK_HEADER_PREFIX}{count}:{first}"));
        for chunk in rest {
            entries.push((*chunk).to_owned());
        }
    }
    entries
}

fn parse_chunk_header(remainder: &str) -> Option<(usize, &str)> {
    let (count_str, first_chunk) = remainder.split_once(':')?;
    let count = count_str.parse::<usize>().ok()?;
    if (1..=KEYRING_MAX_CHUNKS).contains(&count) { Some((count, first_chunk)) } else { None }
}

/// Reassembles a stored payload from primary and continuation entries.
pub fn decode_entries(primary: &str, continuation: &[String]) -> Option<String> {
    let Some(remainder) = primary.strip_prefix(CHUNK_HEADER_PREFIX) else {
        return Some(primary.to_owned());
    };
    let (count, first_chunk) = parse_chunk_header(remainder)?;
    let needed = count - 1;
    if continuation.len() < needed {
        return None;
    }
    let mut result = String::from(first_chunk);
    for piece in &continuation[..needed] {
        result.push_str(piece);
    }
    Some(result)
}

fn parse_stored_count(secret: &str) -> usize {
    secret
        .strip_prefix(CHUNK_HEADER_PREFIX)
        .and_then(|r| r.split_once(':'))
        .and_then(|(count_str, _)| count_str.parse::<usize>().ok())
        .map(|count| count.clamp(1, KEYRING_MAX_CHUNKS))
        .unwrap_or(1)
}

/// Pure helper that cleans pasted cookie headers.
///
/// Trims surrounding whitespace, strips an optional single case-insensitive
/// `cookie:` prefix, and rejects empty remainders.
pub fn sanitize_cookie_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let cleaned = strip_cookie_prefix(trimmed);
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.to_owned())
}

fn strip_cookie_prefix(value: &str) -> &str {
    if let Some(prefix) = value.get(..7) {
        if prefix.eq_ignore_ascii_case("cookie:") {
            return value.get(7..).unwrap_or("").trim_start();
        }
    }
    value
}

/// Session store backed by the platform credential manager via `keyring`.
#[derive(Debug, Clone)]
pub struct KeyringSessionStore {
    service: String,
    account: String,
}

impl KeyringSessionStore {
    /// Creates a store with custom service and account names.
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self { service: service.into(), account: account.into() }
    }

    /// Creates the default application store targeting YouTube Music cookies.
    pub fn moggyt() -> Self {
        Self::new("moggyt", "youtube-music-cookies")
    }

    fn entry_for(&self, index: usize) -> anyhow::Result<keyring::Entry> {
        let account = chunk_account(&self.account, index);
        keyring::Entry::new(&self.service, &account).map_err(Into::into)
    }

    fn stored_entry_count(&self) -> usize {
        self.entry_for(0)
            .ok()
            .and_then(|entry| entry.get_secret().ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .as_deref()
            .map(parse_stored_count)
            .unwrap_or(1)
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn write_entry(&self, index: usize, secret: &str) -> bool {
        let entry = match self.entry_for(index) {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(error = %err, index, "failed to open OS keyring entry for save");
                return false;
            }
        };

        if let Err(err) = entry.set_secret(secret.as_bytes()) {
            tracing::warn!(error = %err, index, "failed to store session cookies in OS keyring");
            return false;
        }

        true
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn delete_entry_quiet(&self, index: usize) {
        let entry = match self.entry_for(index) {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(error = %err, index, "failed to open OS keyring entry for clear");
                return;
            }
        };

        if let Err(err) = entry.delete_credential() {
            if !matches!(err, keyring::Error::NoEntry) {
                tracing::warn!(error = %err, index, "failed to clear session cookies from OS keyring");
            }
        }
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn read_primary_secret(&self) -> Option<String> {
        let entry = match self.entry_for(0) {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(error = %err, "failed to open OS keyring entry for load");
                return None;
            }
        };

        let bytes = match entry.get_secret() {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!(error = %err, "failed to load session cookies from OS keyring");
                return None;
            }
        };

        match String::from_utf8(bytes) {
            Ok(stored) => Some(stored),
            Err(err) => {
                tracing::warn!(error = %err, "stored OS keyring secret is not valid UTF-8");
                None
            }
        }
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn read_continuation(&self, index: usize) -> Option<String> {
        let entry = match self.entry_for(index) {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    index,
                    "failed to open OS keyring continuation entry for load"
                );
                return None;
            }
        };

        let bytes = match entry.get_secret() {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    index,
                    "failed to load session continuation from OS keyring"
                );
                return None;
            }
        };

        match String::from_utf8(bytes) {
            Ok(stored) => Some(stored),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    index,
                    "stored OS keyring continuation is not valid UTF-8"
                );
                None
            }
        }
    }
}

impl SessionStore for KeyringSessionStore {
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn save(&mut self, cookies: &str) {
        let Some(cleaned) = sanitize_cookie_input(cookies) else {
            self.clear();
            return;
        };

        let previous_count = self.stored_entry_count();
        let entries = encode_entries(&cleaned);

        for (index, chunk) in entries.iter().enumerate() {
            if !self.write_entry(index, chunk) {
                return;
            }
        }

        for index in entries.len()..previous_count {
            self.delete_entry_quiet(index);
        }
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn load(&self) -> Option<String> {
        let primary = self.read_primary_secret()?;

        let Some(remainder) = primary.strip_prefix(CHUNK_HEADER_PREFIX) else {
            return Some(primary);
        };

        let Some((count, _)) = parse_chunk_header(remainder) else {
            tracing::warn!("stored OS keyring header is malformed");
            return None;
        };

        let mut continuations = Vec::with_capacity(count.saturating_sub(1));
        for index in 1..count {
            let chunk = self.read_continuation(index)?;
            continuations.push(chunk);
        }

        let decoded = decode_entries(&primary, &continuations);
        if decoded.is_none() {
            tracing::warn!("failed to reassemble chunked session cookies from OS keyring");
        }
        decoded
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn clear(&mut self) {
        let count = self.stored_entry_count();
        for index in 0..count {
            self.delete_entry_quiet(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_cookie_input_table() {
        let cases = [
            ("SID=abc123xyz; HSID=def456", Some("SID=abc123xyz; HSID=def456")),
            ("  SID=abc123xyz  ", Some("SID=abc123xyz")),
            ("cookie: SID=abc123xyz", Some("SID=abc123xyz")),
            ("COOKIE:   SID=abc123xyz", Some("SID=abc123xyz")),
            ("Cookie:SID=abc123xyz", Some("SID=abc123xyz")),
            ("cOoKiE: \t SID=abc123xyz", Some("SID=abc123xyz")),
            ("", None),
            ("   ", None),
            ("\t\r\n", None),
            ("cookie:", None),
            ("COOKIE:   ", None),
            ("cookie:\t\n", None),
        ];

        for (input, expected) in cases {
            assert_eq!(
                sanitize_cookie_input(input).as_deref(),
                expected,
                "failed for input: {input:?}"
            );
        }
    }

    #[test]
    fn keyring_session_store_constructors() {
        let custom = KeyringSessionStore::new("custom-svc", "custom-acc");
        assert_eq!(custom.service, "custom-svc");
        assert_eq!(custom.account, "custom-acc");

        let default_store = KeyringSessionStore::moggyt();
        assert_eq!(default_store.service, "moggyt");
        assert_eq!(default_store.account, "youtube-music-cookies");
    }

    #[test]
    fn keyring_session_store_provider_tolerance_lifecycle() {
        let mut store = KeyringSessionStore::new("moggyt-test", "moggyt-test-probe");
        store.clear();

        store.save("cookie: SID=probe_test_cookie_123");
        let Some(value) = store.load() else {
            // A transient provider failure is indistinguishable from an absent provider
            // through the `SessionStore` port, so only the successful path is asserted.
            eprintln!("keyring provider absent in current environment; tolerated per SDD D-2");
            store.clear();
            return;
        };
        assert_eq!(value, "SID=probe_test_cookie_123");

        store.save("   ");
        assert_eq!(store.load(), None);

        store.clear();
    }

    #[test]
    fn chunk_account_naming() {
        assert_eq!(chunk_account("youtube-music-cookies", 0), "youtube-music-cookies");
        assert_eq!(chunk_account("youtube-music-cookies", 1), "youtube-music-cookies #2");
        assert_eq!(chunk_account("youtube-music-cookies", 2), "youtube-music-cookies #3");
        assert_eq!(chunk_account("custom", 63), "custom #64");
    }

    #[test]
    fn encode_decode_round_trip_small_payload() {
        let payload = "SID=abc123xyz; HSID=def456";
        let entries = encode_entries(payload);
        assert_eq!(entries.len(), 1);
        let decoded = decode_entries(&entries[0], &entries[1..]);
        assert_eq!(decoded.as_deref(), Some(payload));

        let empty_entries = encode_entries("");
        assert_eq!(empty_entries.len(), 1);
        let empty_decoded = decode_entries(&empty_entries[0], &empty_entries[1..]);
        assert_eq!(empty_decoded.as_deref(), Some(""));
    }

    #[test]
    fn encode_decode_round_trip_large_payload() {
        let payload = "a".repeat(5500);
        let entries = encode_entries(&payload);
        assert!(entries.len() > 1);

        let max_first_len = KEYRING_CHUNK_BYTES + CHUNK_HEADER_PREFIX.len() + 4;
        for (index, entry) in entries.iter().enumerate() {
            if index == 0 {
                assert!(
                    entry.len() <= max_first_len,
                    "first entry length {} exceeds bound {}",
                    entry.len(),
                    max_first_len
                );
            } else {
                assert!(
                    entry.len() <= KEYRING_CHUNK_BYTES,
                    "continuation entry {index} length {} exceeds bound {}",
                    entry.len(),
                    KEYRING_CHUNK_BYTES
                );
            }
        }

        let decoded = decode_entries(&entries[0], &entries[1..]);
        assert_eq!(decoded.as_deref(), Some(payload.as_str()));
    }

    #[test]
    fn encode_decode_round_trip_multibyte_utf8() {
        let unit = "caf\u{00e9}";
        // unit is 5 bytes: 'c', 'a', 'f' (3 bytes) + 2-byte '\u{00e9}'.
        // With a 1-byte prefix 'X', offset 2000 falls at (2000 - 1) % 5 = 4,
        // which lands on the continuation byte of '\u{00e9}', not a char boundary.
        let payload = format!("X{}", unit.repeat(700));
        assert!(!payload.is_char_boundary(KEYRING_CHUNK_BYTES));

        let entries = encode_entries(&payload);
        assert!(entries.len() > 1);
        let decoded = decode_entries(&entries[0], &entries[1..]);
        assert_eq!(decoded.as_deref(), Some(payload.as_str()));
    }

    #[test]
    fn decode_legacy_passthrough() {
        let legacy = "SID=legacy_secret; HSID=token123";
        let result = decode_entries(legacy, &[]);
        assert_eq!(result.as_deref(), Some(legacy));

        let with_continuations = decode_entries(legacy, &["extra".to_owned()]);
        assert_eq!(with_continuations.as_deref(), Some(legacy));
    }

    #[test]
    fn decode_rejects_malformed_header() {
        assert_eq!(decode_entries("moggyt-chunked-v1:", &[]), None);
        assert_eq!(decode_entries("moggyt-chunked-v1:0:x", &[]), None);
        assert_eq!(decode_entries("moggyt-chunked-v1:abc:x", &[]), None);
        assert_eq!(decode_entries("moggyt-chunked-v1:999:x", &[]), None);
    }

    #[test]
    fn decode_rejects_missing_continuations() {
        let primary = "moggyt-chunked-v1:3:first";
        assert_eq!(decode_entries(primary, &[]), None);
        assert_eq!(decode_entries(primary, &["second".to_owned()]), None);
        assert_eq!(
            decode_entries(primary, &["second".to_owned(), "third".to_owned()]).as_deref(),
            Some("firstsecondthird")
        );
    }

    #[test]
    fn entry_size_boundary() {
        let exactly_chunk = "a".repeat(KEYRING_CHUNK_BYTES);
        let entries_exact = encode_entries(&exactly_chunk);
        assert_eq!(entries_exact.len(), 1);
        assert_eq!(
            decode_entries(&entries_exact[0], &entries_exact[1..]).as_deref(),
            Some(exactly_chunk.as_str())
        );

        let one_more = "a".repeat(KEYRING_CHUNK_BYTES + 1);
        let entries_one_more = encode_entries(&one_more);
        assert_eq!(entries_one_more.len(), 2);
        assert_eq!(
            decode_entries(&entries_one_more[0], &entries_one_more[1..]).as_deref(),
            Some(one_more.as_str())
        );
    }

    #[test]
    fn parse_stored_count_clamped() {
        assert_eq!(parse_stored_count("legacy-cookie"), 1);
        assert_eq!(parse_stored_count("moggyt-chunked-v1:"), 1);
        assert_eq!(parse_stored_count("moggyt-chunked-v1:abc:x"), 1);
        assert_eq!(parse_stored_count("moggyt-chunked-v1:0:x"), 1);
        assert_eq!(parse_stored_count("moggyt-chunked-v1:3:x"), 3);
        assert_eq!(parse_stored_count("moggyt-chunked-v1:999:x"), KEYRING_MAX_CHUNKS);
    }
}
