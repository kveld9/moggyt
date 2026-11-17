//! Google session cookie rotation for the `SIDTS` freshness tokens.
//!
//! YouTube answers as anonymous once `__Secure-1PSIDTS` / `__Secure-3PSIDTS`
//! go stale. The accounts rotation endpoint issues fresh values while the
//! session is still valid, which is the same mechanism Chrome uses.

/// Rotation endpoint for YouTube session cookies.
const ROTATE_URL: &str = "https://accounts.youtube.com/RotateCookies";

/// Sentinel body the rotation endpoint expects.
const ROTATE_BODY: &str = r#"[000,"-0000000000000000000"]"#;

/// Requests fresh session tokens and returns the merged cookie header.
pub fn rotate_cookies(client: &reqwest::blocking::Client, cookies: &str) -> anyhow::Result<String> {
    let response = client
        .post(ROTATE_URL)
        .header("Content-Type", "application/json")
        .header("Origin", "https://accounts.youtube.com")
        .header("Cookie", cookies)
        .body(ROTATE_BODY)
        .send()?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("session rotation failed with status: {status}");
    }
    let rotated: Vec<String> = response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::to_owned)
        .collect();
    Ok(merge_set_cookies(cookies, rotated.iter().map(String::as_str)))
}

/// Merges `Set-Cookie` pairs into an existing cookie header.
///
/// Replaces cookies already present by name, appends new ones, and keeps every
/// other cookie untouched so authentication cookies survive rotation.
pub fn merge_set_cookies<'a>(cookies: &str, set_cookies: impl Iterator<Item = &'a str>) -> String {
    let mut pairs: Vec<(String, String)> = cookies
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .filter(|(name, _)| !name.is_empty())
        .collect();
    for raw in set_cookies {
        let Some(pair) = raw.split(';').next() else {
            continue;
        };
        let Some((name, value)) = pair.trim().split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        match pairs.iter_mut().find(|(existing, _)| existing == name) {
            Some(existing) => existing.1 = value.to_owned(),
            None => pairs.push((name.to_owned(), value.to_owned())),
        }
    }
    pairs.into_iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_replaces_rotated_tokens_and_keeps_session_cookies() {
        let cookies = "SID=sid; SAPISID=sap; __Secure-1PSIDTS=old1; __Secure-3PSIDTS=old3";
        let merged = merge_set_cookies(
            cookies,
            [
                "__Secure-1PSIDTS=new1; Domain=.youtube.com; Path=/; Secure",
                "__Secure-3PSIDTS=new3; Domain=.youtube.com; Path=/; Secure",
            ]
            .into_iter(),
        );
        assert_eq!(merged, "SID=sid; SAPISID=sap; __Secure-1PSIDTS=new1; __Secure-3PSIDTS=new3");
    }

    #[test]
    fn merge_appends_unknown_cookies_and_ignores_malformed_pairs() {
        let merged = merge_set_cookies(
            "SID=sid",
            ["SIDCC=fresh; Path=/", "garbage", "=novalue", "EMPTY=; Path=/"].into_iter(),
        );
        assert_eq!(merged, "SID=sid; SIDCC=fresh");
    }
}
