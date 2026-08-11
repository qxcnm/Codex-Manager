use std::fmt;

use http::{header, HeaderMap, HeaderName, HeaderValue};
use thiserror::Error;

const X_XAI_REQUEST_ID: HeaderName = HeaderName::from_static("x-xai-request-id");
const X_STATSIG_ID: HeaderName = HeaderName::from_static("x-statsig-id");

/// Opaque Grok SSO bearer value. Its debug output is always redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretSsoToken(String);

impl SecretSsoToken {
    pub fn parse(value: impl Into<String>) -> Result<Self, GrokHeaderError> {
        let mut value = value.into();
        value = value.trim().to_owned();
        if value
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sso="))
        {
            value = value[4..].trim().to_owned();
        }
        if let Some((token, _)) = value.split_once(';') {
            value = token.trim().to_owned();
        }
        if value.is_empty() {
            return Err(GrokHeaderError::EmptySsoToken);
        }
        if value
            .bytes()
            .any(|byte| byte <= 0x20 || byte == b';' || byte == b',')
        {
            return Err(GrokHeaderError::InvalidSsoToken);
        }
        Ok(Self(value))
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretSsoToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretSsoToken([REDACTED])")
    }
}

/// Browser-facing header identity. TLS impersonation must use the same browser
/// major version as this user agent at the HTTP client boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokWebHeaderProfile {
    pub user_agent: String,
    pub accept_language: String,
    pub origin: String,
    pub referer: String,
}

impl Default for GrokWebHeaderProfile {
    fn default() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36".to_owned(),
            accept_language: "zh-CN,zh;q=0.9,en;q=0.8".to_owned(),
            origin: "https://grok.com".to_owned(),
            referer: "https://grok.com/".to_owned(),
        }
    }
}

/// Wrapper whose debug representation cannot accidentally disclose cookies.
pub struct SensitiveHeaders(HeaderMap);

impl SensitiveHeaders {
    pub fn as_headers(&self) -> &HeaderMap {
        &self.0
    }

    pub fn into_inner(self) -> HeaderMap {
        self.0
    }
}

impl fmt::Debug for SensitiveHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<_> = self.0.keys().map(HeaderName::as_str).collect();
        formatter
            .debug_struct("SensitiveHeaders")
            .field("names", &names)
            .field("values", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GrokHeaderError {
    #[error("Grok SSO token is empty")]
    EmptySsoToken,
    #[error("Grok SSO token contains invalid cookie characters")]
    InvalidSsoToken,
    #[error("invalid header value for {0}")]
    InvalidHeader(&'static str),
    #[error("invalid Cloudflare cookie string")]
    InvalidCloudflareCookies,
}

pub fn build_web_headers(
    token: &SecretSsoToken,
    profile: &GrokWebHeaderProfile,
    request_id: &str,
    statsig_id: Option<&str>,
    cloudflare_cookies: Option<&str>,
) -> Result<SensitiveHeaders, GrokHeaderError> {
    let mut headers = HeaderMap::new();
    insert(
        &mut headers,
        header::CONTENT_TYPE,
        "application/json",
        "content-type",
    )?;
    insert(&mut headers, header::ACCEPT, "*/*", "accept")?;
    insert(
        &mut headers,
        header::ACCEPT_ENCODING,
        "gzip, deflate, br, zstd",
        "accept-encoding",
    )?;
    insert(
        &mut headers,
        header::ACCEPT_LANGUAGE,
        &profile.accept_language,
        "accept-language",
    )?;
    insert(
        &mut headers,
        header::USER_AGENT,
        &profile.user_agent,
        "user-agent",
    )?;
    insert(&mut headers, header::ORIGIN, &profile.origin, "origin")?;
    insert(&mut headers, header::REFERER, &profile.referer, "referer")?;
    insert(
        &mut headers,
        header::CACHE_CONTROL,
        "no-cache",
        "cache-control",
    )?;
    insert(&mut headers, header::PRAGMA, "no-cache", "pragma")?;
    insert(
        &mut headers,
        HeaderName::from_static("priority"),
        "u=1, i",
        "priority",
    )?;
    insert(
        &mut headers,
        HeaderName::from_static("sec-fetch-dest"),
        "empty",
        "sec-fetch-dest",
    )?;
    insert(
        &mut headers,
        HeaderName::from_static("sec-fetch-mode"),
        "cors",
        "sec-fetch-mode",
    )?;
    insert(
        &mut headers,
        HeaderName::from_static("sec-fetch-site"),
        "same-origin",
        "sec-fetch-site",
    )?;
    insert(
        &mut headers,
        X_XAI_REQUEST_ID,
        request_id,
        "x-xai-request-id",
    )?;
    if let Some(value) = statsig_id.filter(|value| !value.trim().is_empty()) {
        insert(&mut headers, X_STATSIG_ID, value, "x-statsig-id")?;
    }

    let mut cookie = format!("sso={0}; sso-rw={0}", token.expose());
    if let Some(extra) = sanitize_cloudflare_cookies(cloudflare_cookies)? {
        cookie.push_str("; ");
        cookie.push_str(&extra);
    }
    insert(&mut headers, header::COOKIE, &cookie, "cookie")?;
    Ok(SensitiveHeaders(headers))
}

fn insert(
    headers: &mut HeaderMap,
    name: HeaderName,
    value: &str,
    label: &'static str,
) -> Result<(), GrokHeaderError> {
    let value = HeaderValue::from_str(value).map_err(|_| GrokHeaderError::InvalidHeader(label))?;
    headers.insert(name, value);
    Ok(())
}

fn sanitize_cloudflare_cookies(value: Option<&str>) -> Result<Option<String>, GrokHeaderError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let mut accepted = Vec::new();
    for pair in value.split(';') {
        let (name, value) = pair
            .trim()
            .split_once('=')
            .ok_or(GrokHeaderError::InvalidCloudflareCookies)?;
        let name = name.trim();
        let value = value.trim();
        if !matches!(name, "cf_clearance" | "__cf_bm" | "_cfuvid") {
            continue;
        }
        if value.is_empty()
            || value
                .bytes()
                .any(|byte| byte <= 0x20 || byte == b';' || byte == b',')
        {
            return Err(GrokHeaderError::InvalidCloudflareCookies);
        }
        accepted.push(format!("{name}={value}"));
    }
    Ok((!accepted.is_empty()).then(|| accepted.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_contains_both_sso_names_and_sanitized_cloudflare_values() {
        let token = SecretSsoToken::parse("sso=header.payload.signature; Path=/").unwrap();
        let headers = build_web_headers(
            &token,
            &GrokWebHeaderProfile::default(),
            "request-id",
            Some("statsig"),
            Some("cf_clearance=clear; ignored=value; __cf_bm=bm"),
        )
        .unwrap();
        assert_eq!(
            headers.as_headers()[header::COOKIE],
            "sso=header.payload.signature; sso-rw=header.payload.signature; cf_clearance=clear; __cf_bm=bm"
        );
    }

    #[test]
    fn debug_output_never_contains_the_token() {
        let token = SecretSsoToken::parse("secret.jwt.value").unwrap();
        assert!(!format!("{token:?}").contains("secret.jwt.value"));
        let headers = build_web_headers(
            &token,
            &GrokWebHeaderProfile::default(),
            "request-id",
            None,
            None,
        )
        .unwrap();
        assert!(!format!("{headers:?}").contains("secret.jwt.value"));
    }

    #[test]
    fn rejects_cookie_injection() {
        assert_eq!(
            SecretSsoToken::parse("token\r\nX-Evil: yes").unwrap_err(),
            GrokHeaderError::InvalidSsoToken
        );
    }
}
