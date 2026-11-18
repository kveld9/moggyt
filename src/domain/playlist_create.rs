//! Pure domain: validation of new playlist title.
//!
//! Invariant: this module performs no I/O.

/// Maximum number of characters allowed in a playlist title by the catalog API.
pub const MAX_PLAYLIST_TITLE_CHARS: usize = 150;

/// Failure conditions when validating a playlist title for creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistTitleError {
    /// Title is empty or contains only whitespace.
    Empty,
    /// Title exceeds the maximum allowed character length.
    TooLong,
    /// Title contains characters unsupported by the catalog (`<` or `>`).
    UnsupportedCharacters,
}

impl std::fmt::Display for PlaylistTitleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "playlist title cannot be empty"),
            Self::TooLong => write!(f, "playlist title exceeds maximum length"),
            Self::UnsupportedCharacters => {
                write!(f, "playlist title contains unsupported characters")
            }
        }
    }
}

impl std::error::Error for PlaylistTitleError {}

fn has_unsupported_chars(s: &str) -> bool {
    s.contains('<') || s.contains('>')
}

/// Validates and trims a proposed playlist title for creation.
pub fn new_playlist_title(title: &str) -> Result<String, PlaylistTitleError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(PlaylistTitleError::Empty);
    }
    if trimmed.chars().count() > MAX_PLAYLIST_TITLE_CHARS {
        return Err(PlaylistTitleError::TooLong);
    }
    if has_unsupported_chars(trimmed) {
        return Err(PlaylistTitleError::UnsupportedCharacters);
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_playlist_title_table() {
        let cases: &[(&str, Result<&str, PlaylistTitleError>)] = &[
            ("", Err(PlaylistTitleError::Empty)),
            ("   ", Err(PlaylistTitleError::Empty)),
            ("  Mix  ", Ok("Mix")),
            ("a<b", Err(PlaylistTitleError::UnsupportedCharacters)),
            ("a>b", Err(PlaylistTitleError::UnsupportedCharacters)),
            ("<title>", Err(PlaylistTitleError::UnsupportedCharacters)),
        ];

        for &(input, expected) in cases {
            let res = new_playlist_title(input);
            match expected {
                Ok(expected_str) => assert_eq!(res.as_deref(), Ok(expected_str)),
                Err(expected_err) => assert_eq!(res, Err(expected_err)),
            }
        }
    }

    #[test]
    fn test_new_playlist_title_char_length_boundary() {
        let ascii_150 = "a".repeat(150);
        assert_eq!(new_playlist_title(&ascii_150), Ok(ascii_150));

        let ascii_151 = "a".repeat(151);
        assert_eq!(new_playlist_title(&ascii_151), Err(PlaylistTitleError::TooLong));

        let pattern_150 = "ab".repeat(75);
        assert_eq!(new_playlist_title(&pattern_150), Ok(pattern_150));

        let pattern_151 = format!("{}a", "ab".repeat(75));
        assert_eq!(new_playlist_title(&pattern_151), Err(PlaylistTitleError::TooLong));

        // Multibyte boundary check with 2-byte characters represented via ASCII escapes
        let multibyte_150 = "\u{00e9}".repeat(150);
        assert_eq!(new_playlist_title(&multibyte_150), Ok(multibyte_150));

        let multibyte_151 = "\u{00e9}".repeat(151);
        assert_eq!(new_playlist_title(&multibyte_151), Err(PlaylistTitleError::TooLong));
    }

    #[test]
    fn test_playlist_title_error_display_and_traits() {
        assert_eq!(PlaylistTitleError::Empty.to_string(), "playlist title cannot be empty");
        assert_eq!(
            PlaylistTitleError::TooLong.to_string(),
            "playlist title exceeds maximum length"
        );
        assert_eq!(
            PlaylistTitleError::UnsupportedCharacters.to_string(),
            "playlist title contains unsupported characters"
        );
        assert_eq!(PlaylistTitleError::Empty, PlaylistTitleError::Empty);
        assert_ne!(PlaylistTitleError::Empty, PlaylistTitleError::TooLong);
    }
}
