//! Companion device session-handoff protocol payload envelope.
//!
//! Provides pure, versioned encoding and decoding of session cookies for QR-code
//! and copy-paste handoff from companion devices (e.g. mobile or headless setups)
//! to the desktop client.
//!
//! # Envelope Format
//! `MG1.<base64-payload>.<checksum-hex>`
//! - `MG1`: Envelope prefix indicating moggyt handoff format version 1.
//! - `<base64-payload>`: RFC 4648 standard Base64-encoded cookie string.
//! - `<checksum-hex>`: Truncated 8-character lowercase hexadecimal MD5 digest
//!   computed over the `<base64-payload>` string.
//!
//! # Checksum Role & Security Model
//! The checksum serves strictly as an accidental transmission corruption and typo
//! detector (for example, camera scan noise, truncation during copy-paste, or visual
//! glitches). It is explicitly NOT a cryptographic authentication or tamper-proofing
//! guarantee against intentional adversarial modification.

use md5::{Digest, Md5};

use super::sanitize_cookie_input;

/// Current handoff envelope format prefix identifying version 1.
pub const HANDOFF_PREFIX: &str = "MG1";

/// Maximum permitted length of raw cookie string in characters (8 KiB).
/// Prevents denial-of-service and memory abuse via oversized handoff codes.
pub const MAX_HANDOFF_COOKIES: usize = 8192;

/// Length in hex characters of the truncated MD5 checksum.
///
/// Role: Accidental typo and transmission corruption detection.
/// Not a cryptographic security boundary.
pub const CHECKSUM_HEX_LEN: usize = 8;

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Minimal private standard RFC 4648 Base64 encoder.
///
/// Scope: Internal pure encoding for companion handoff envelopes without
/// requiring external base64 crate dependencies.
fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let chunks = input.chunks_exact(3);
    let remainder = chunks.remainder();

    for chunk in chunks {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        out.push(BASE64_ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        out.push(BASE64_ALPHABET[(n & 0x3F) as usize] as char);
    }

    match remainder.len() {
        1 => {
            let n = (remainder[0] as u32) << 16;
            out.push(BASE64_ALPHABET[((n >> 18) & 0x3F) as usize] as char);
            out.push(BASE64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = ((remainder[0] as u32) << 16) | ((remainder[1] as u32) << 8);
            out.push(BASE64_ALPHABET[((n >> 18) & 0x3F) as usize] as char);
            out.push(BASE64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
            out.push(BASE64_ALPHABET[((n >> 6) & 0x3F) as usize] as char);
            out.push('=');
        }
        _ => {}
    }

    out
}

/// Minimal private standard RFC 4648 Base64 character decoder.
fn decode_char(b: u8) -> anyhow::Result<u8> {
    match b {
        b'A'..=b'Z' => Ok(b - b'A'),
        b'a'..=b'z' => Ok(b - b'a' + 26),
        b'0'..=b'9' => Ok(b - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => anyhow::bail!("invalid base64 character: {}", b as char),
    }
}

fn decode_full_chunk(chunk: &[u8], out: &mut Vec<u8>) -> anyhow::Result<()> {
    let d0 = decode_char(chunk[0])?;
    let d1 = decode_char(chunk[1])?;
    let d2 = decode_char(chunk[2])?;
    let d3 = decode_char(chunk[3])?;

    out.push((d0 << 2) | (d1 >> 4));
    out.push((d1 << 4) | (d2 >> 2));
    out.push((d2 << 6) | d3);
    Ok(())
}

fn decode_pad1(chunk: &[u8], out: &mut Vec<u8>) -> anyhow::Result<()> {
    let d0 = decode_char(chunk[0])?;
    let d1 = decode_char(chunk[1])?;
    let d2 = decode_char(chunk[2])?;
    if (d2 & 0x03) != 0 {
        anyhow::bail!("invalid base64 padding bits in last quantum");
    }
    out.push((d0 << 2) | (d1 >> 4));
    out.push((d1 << 4) | (d2 >> 2));
    Ok(())
}

fn decode_pad2(chunk: &[u8], out: &mut Vec<u8>) -> anyhow::Result<()> {
    let d0 = decode_char(chunk[0])?;
    let d1 = decode_char(chunk[1])?;
    if (d1 & 0x0F) != 0 {
        anyhow::bail!("invalid base64 padding bits in last quantum");
    }
    out.push((d0 << 2) | (d1 >> 4));
    Ok(())
}

fn decode_final_chunk(chunk: &[u8], out: &mut Vec<u8>) -> anyhow::Result<()> {
    if chunk[2] == b'=' && chunk[3] == b'=' {
        return decode_pad2(chunk, out);
    }
    if chunk[3] == b'=' {
        return decode_pad1(chunk, out);
    }
    if chunk[2] == b'=' {
        anyhow::bail!("invalid base64 padding sequence");
    }
    decode_full_chunk(chunk, out)
}

/// Minimal private standard RFC 4648 Base64 decoder.
///
/// Rejects non-alphabet characters, invalid lengths, misplaced padding,
/// and trailing non-zero padding bits loudly.
fn base64_decode(input: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.len() % 4 != 0 {
        anyhow::bail!("invalid base64 length: not a multiple of 4");
    }

    let chunk_count = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunk_count * 3);

    for i in 0..(chunk_count - 1) {
        let chunk = &bytes[i * 4..(i + 1) * 4];
        if chunk.contains(&b'=') {
            anyhow::bail!("unexpected base64 padding before end of payload");
        }
        decode_full_chunk(chunk, &mut out)?;
    }

    let last_chunk = &bytes[(chunk_count - 1) * 4..];
    decode_final_chunk(last_chunk, &mut out)?;

    Ok(out)
}

/// Computes truncated MD5 checksum over the base64 payload.
fn compute_checksum(payload: &str) -> String {
    let digest = Md5::digest(payload.as_bytes());
    let hex = format!("{digest:x}");
    hex[..CHECKSUM_HEX_LEN].to_ascii_lowercase()
}

/// Constant-shape comparison for the truncated MD5 checksum.
///
/// Compares expected and actual hex strings character-by-character.
/// Note: Accidental corruption and typo detector, not a timing-attack defense.
fn verify_checksum(expected: &str, actual: &str) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    expected.eq_ignore_ascii_case(actual)
}

/// Encodes raw cookie input into a versioned, checksummed handoff envelope.
///
/// Sanitizes cookies via [`sanitize_cookie_input`], rejecting blank input
/// and sizes exceeding [`MAX_HANDOFF_COOKIES`] loudly.
pub fn encode_handoff(cookies: &str) -> anyhow::Result<String> {
    let Some(cleaned) = sanitize_cookie_input(cookies) else {
        anyhow::bail!("cannot encode empty or blank cookie input");
    };
    if cleaned.len() > MAX_HANDOFF_COOKIES {
        anyhow::bail!(
            "cookie payload exceeds maximum cap of {} bytes (got {})",
            MAX_HANDOFF_COOKIES,
            cleaned.len()
        );
    }
    let payload = base64_encode(cleaned.as_bytes());
    let checksum = compute_checksum(&payload);
    Ok(format!("{HANDOFF_PREFIX}.{payload}.{checksum}"))
}

fn parse_envelope(input: &str) -> anyhow::Result<(&str, &str)> {
    let parts: Vec<&str> = input.split('.').collect();
    if parts.len() != 3 {
        anyhow::bail!("invalid handoff envelope shape: expected 3 dot-separated parts");
    }
    if parts[0] != HANDOFF_PREFIX {
        anyhow::bail!("unsupported handoff envelope prefix: expected '{HANDOFF_PREFIX}'");
    }
    Ok((parts[1], parts[2]))
}

fn verify_envelope_checksum(payload: &str, checksum: &str) -> anyhow::Result<()> {
    let expected = compute_checksum(payload);
    if !verify_checksum(&expected, checksum) {
        anyhow::bail!("checksum mismatch in handoff envelope");
    }
    Ok(())
}

fn parse_payload_utf8(raw_bytes: &[u8]) -> anyhow::Result<String> {
    String::from_utf8(raw_bytes.to_vec())
        .map_err(|err| anyhow::anyhow!("decoded handoff payload is not valid UTF-8: {err}"))
}

fn clean_and_validate_cookies(decoded: &str) -> anyhow::Result<String> {
    let Some(cleaned) = sanitize_cookie_input(decoded) else {
        anyhow::bail!("decoded handoff payload contains blank or empty cookies");
    };
    if cleaned.len() > MAX_HANDOFF_COOKIES {
        anyhow::bail!(
            "decoded cookies exceed maximum cap of {} bytes (got {})",
            MAX_HANDOFF_COOKIES,
            cleaned.len()
        );
    }
    Ok(cleaned)
}

/// Decodes and validates a companion handoff code into cleaned session cookies.
///
/// Steps:
/// 1. Trims surrounding whitespace.
/// 2. Verifies `MG1.` prefix and three-part shape.
/// 3. Verifies checksum against payload.
/// 4. Decodes standard Base64 payload.
/// 5. Verifies decoded cookie content is non-blank per [`sanitize_cookie_input`]
///    and within [`MAX_HANDOFF_COOKIES`].
pub fn decode_handoff(code: &str) -> anyhow::Result<String> {
    let trimmed = code.trim();
    let (payload, checksum) = parse_envelope(trimmed)?;
    verify_envelope_checksum(payload, checksum)?;
    let raw_bytes = base64_decode(payload)?;
    let decoded_str = parse_payload_utf8(&raw_bytes)?;
    clean_and_validate_cookies(&decoded_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_decode_round_trip_padding_cases() {
        let cases: &[&[u8]] = &[
            b"",
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"fooba",
            b"foobar",
            b"SID=abc123xyz; HSID=def456uvw",
            &[0x00, 0xFF, 0x80, 0x7F, 0x55, 0xAA, 0x01, 0x02, 0x03],
        ];

        for &bytes in cases {
            let encoded = base64_encode(bytes);
            let decoded = base64_decode(&encoded).expect("decode failed");
            assert_eq!(decoded, bytes, "round-trip failed for: {bytes:?}");
        }

        // Verify standard RFC 4648 padding lengths
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn test_base64_decode_rejects_invalid_alphabet_and_padding() {
        let invalid_cases = [
            "Zm9!",  // Invalid character '!'
            "Zm 9v", // Space character
            "Zm9\n", // Newline character
            "Zm-v",  // URL-safe '-' not in standard alphabet
            "Zm_v",  // URL-safe '_' not in standard alphabet
            "Z",     // Length mod 4 == 1
            "Zm",    // Length mod 4 == 2
            "Zm9",   // Length mod 4 == 3
            "Zm9vv", // Length mod 4 == 1
            "Z=9v",  // Padding in middle of quantum
            "====",  // Only padding characters
            "Zm=v",  // Padding before non-padding in last quantum
            "Zm=A",  // Padding followed by char
            "YR==",  // Non-zero unused bits in 2-pad quantum
            "YWJ=",  // Non-zero unused bits in 1-pad quantum
        ];

        for &invalid in &invalid_cases {
            assert!(
                base64_decode(invalid).is_err(),
                "expected invalid base64 to be rejected: {invalid}"
            );
        }
    }

    #[test]
    fn test_handoff_envelope_round_trip() {
        let raw_cookies = "SID=abc123xyz; HSID=def456uvw; expires=Wed, 21 Oct 2026";
        let envelope = encode_handoff(raw_cookies).expect("encode failed");

        assert!(envelope.starts_with("MG1."));
        let parts: Vec<&str> = envelope.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "MG1");
        assert_eq!(parts[2].len(), CHECKSUM_HEX_LEN);

        let decoded = decode_handoff(&envelope).expect("decode failed");
        assert_eq!(decoded, raw_cookies);

        // Case-insensitive 'cookie:' prefix stripped on encode
        let prefixed = format!("cookie:   {raw_cookies}");
        let envelope_prefixed = encode_handoff(&prefixed).expect("encode prefixed failed");
        let decoded_prefixed = decode_handoff(&envelope_prefixed).expect("decode prefixed failed");
        assert_eq!(decoded_prefixed, raw_cookies);
    }

    #[test]
    fn test_handoff_tampered_payload_and_checksum_rejected() {
        let envelope = encode_handoff("SID=valid_cookie").expect("encode");
        let parts: Vec<&str> = envelope.split('.').collect();

        // Tamper base64 payload
        let tampered_payload =
            format!("{}.{}x.{}", parts[0], &parts[1][..parts[1].len() - 1], parts[2]);
        assert!(decode_handoff(&tampered_payload).is_err());

        // Tamper checksum character
        let bad_char = if parts[2].starts_with('0') { '1' } else { '0' };
        let tampered_checksum = format!("{}.{}.{}{}", parts[0], parts[1], bad_char, &parts[2][1..]);
        assert!(decode_handoff(&tampered_checksum).is_err());

        // Truncated / wrong length checksum
        let short_checksum =
            format!("{}.{}.{}", parts[0], parts[1], &parts[2][..CHECKSUM_HEX_LEN - 1]);
        assert!(decode_handoff(&short_checksum).is_err());

        // Checksum case-insensitivity tolerance
        let uppercase_checksum =
            format!("{}.{}.{}", parts[0], parts[1], parts[2].to_ascii_uppercase());
        assert_eq!(
            decode_handoff(&uppercase_checksum).expect("decode uppercase"),
            "SID=valid_cookie"
        );
    }

    #[test]
    fn test_handoff_wrong_prefix_or_version_rejected() {
        let envelope = encode_handoff("SID=valid_cookie").expect("encode");
        let parts: Vec<&str> = envelope.split('.').collect();

        assert!(decode_handoff(&format!("MG2.{}.{}", parts[1], parts[2])).is_err());
        assert!(decode_handoff(&format!("MG.{}.{}", parts[1], parts[2])).is_err());
        assert!(decode_handoff(&format!("{}.{}", parts[1], parts[2])).is_err());
        assert!(decode_handoff(&format!("{}.{}.{}.extra", parts[0], parts[1], parts[2])).is_err());
        assert!(decode_handoff("").is_err());
        assert!(decode_handoff("   ").is_err());
    }

    #[test]
    fn test_handoff_blank_and_oversize_rejected() {
        // Blank input rejection in encode
        assert!(encode_handoff("").is_err());
        assert!(encode_handoff("   ").is_err());
        assert!(encode_handoff("cookie:").is_err());
        assert!(encode_handoff("cookie:   \t\n").is_err());

        // Exactly at cap succeeds
        let max_str = "a".repeat(MAX_HANDOFF_COOKIES);
        let ok_envelope = encode_handoff(&max_str);
        assert!(ok_envelope.is_ok());
        assert_eq!(decode_handoff(&ok_envelope.unwrap()).unwrap(), max_str);

        // Exceeding cap fails in encode
        let over_str = "a".repeat(MAX_HANDOFF_COOKIES + 1);
        assert!(encode_handoff(&over_str).is_err());

        // Oversized decoded payload rejected in decode
        let raw_oversize = base64_encode(over_str.as_bytes());
        let checksum = compute_checksum(&raw_oversize);
        let oversize_envelope = format!("MG1.{raw_oversize}.{checksum}");
        assert!(decode_handoff(&oversize_envelope).is_err());

        // Blank decoded payload rejected in decode
        let raw_blank = base64_encode(b"   ");
        let checksum_blank = compute_checksum(&raw_blank);
        let blank_envelope = format!("MG1.{raw_blank}.{checksum_blank}");
        assert!(decode_handoff(&blank_envelope).is_err());
    }
}
