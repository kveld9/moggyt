//! Parses the `playlist/create` response payload.

use serde_json::Value;

/// Extracts the created playlist identifier from a `playlist/create` response payload.
///
/// Returns `None` when `"playlistId"` is missing, not a string, or contains only whitespace.
pub fn parse_created_playlist_id(value: &Value) -> Option<String> {
    let raw = value.get("playlistId")?.as_str()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_created_playlist_id_valid() {
        let payload = serde_json::json!({
            "playlistId": "VLPLtest123"
        });
        assert_eq!(parse_created_playlist_id(&payload), Some("VLPLtest123".to_owned()));

        let payload_spaces = serde_json::json!({
            "playlistId": "  PLtest456  "
        });
        assert_eq!(parse_created_playlist_id(&payload_spaces), Some("PLtest456".to_owned()));
    }

    #[test]
    fn parse_created_playlist_id_missing_key() {
        let payload = serde_json::json!({
            "status": "STATUS_SUCCEEDED"
        });
        assert_eq!(parse_created_playlist_id(&payload), None);
    }

    #[test]
    fn parse_created_playlist_id_blank() {
        let payload_empty = serde_json::json!({
            "playlistId": ""
        });
        assert_eq!(parse_created_playlist_id(&payload_empty), None);

        let payload_whitespace = serde_json::json!({
            "playlistId": "   "
        });
        assert_eq!(parse_created_playlist_id(&payload_whitespace), None);
    }

    #[test]
    fn parse_created_playlist_id_numeric() {
        let payload = serde_json::json!({
            "playlistId": 12345
        });
        assert_eq!(parse_created_playlist_id(&payload), None);
    }

    #[test]
    fn parse_created_playlist_id_error_payload() {
        let payload = serde_json::json!({
            "error": {
                "code": 400
            }
        });
        assert_eq!(parse_created_playlist_id(&payload), None);
    }
}
