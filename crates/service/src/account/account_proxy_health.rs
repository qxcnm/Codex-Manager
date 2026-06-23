use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Instant;
use url::{Host, Url};

const STATUS_FAILED: &str = "failed";
const STATUS_INVALID_URL: &str = "invalid_url";
const STATUS_OK: &str = "ok";
const STATUS_RUNTIME_ERROR: &str = "runtime_error";
const IPWHOIS_ENDPOINT: &str = "https://ipwho.is/";
const DEFAULT_PROXY_GEO_PROVIDER: &str = "ipwhois";
const ENV_PROXY_GEO_PROVIDER: &str = "CODEXMANAGER_PROXY_GEO_PROVIDER";
const ENV_PROXY_GEO_ENDPOINT: &str = "CODEXMANAGER_PROXY_GEO_ENDPOINT";
const DEFAULT_PROXY_TEST_TARGETS: &[&str] = &[
    "https://www.gstatic.com/generate_204",
    "https://api.ipify.org",
    "https://chatgpt.com/cdn-cgi/trace",
];

#[derive(Debug, Clone, Deserialize)]
struct IpWhoIsResponse {
    success: bool,
    ip: Option<String>,

    country: Option<String>,
    country_code: Option<String>,
    region: Option<String>,
    city: Option<String>,

    flag: Option<IpWhoIsFlag>,
    connection: Option<IpWhoIsConnection>,
    timezone: Option<IpWhoIsTimezone>,

    message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct IpWhoIsFlag {
    img: Option<String>,
    emoji: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct IpWhoIsConnection {
    asn: Option<i64>,
    org: Option<String>,
    isp: Option<String>,
    domain: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct IpWhoIsTimezone {
    id: Option<String>,
    offset: Option<i64>,
    utc: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProxyGeoInfo {
    pub ip: Option<String>,
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub region_name: Option<String>,
    pub city_name: Option<String>,
    pub geo_checked_at: Option<i64>,
    pub geo_error: Option<String>,

    pub asn: Option<i64>,
    pub as_org: Option<String>,
    pub isp: Option<String>,
    pub as_domain: Option<String>,

    pub timezone_id: Option<String>,
    pub timezone_offset: Option<i64>,
    pub timezone_utc: Option<String>,

    pub flag_img_url: Option<String>,
    pub flag_emoji: Option<String>,
}

impl ProxyGeoInfo {
    fn error(error: String) -> Self {
        Self {
            ip: None,
            country_code: None,
            country_name: None,
            region_name: None,
            city_name: None,
            geo_checked_at: Some(codexmanager_core::storage::now_ts()),
            geo_error: Some(error),
            asn: None,
            as_org: None,
            isp: None,
            as_domain: None,
            timezone_id: None,
            timezone_offset: None,
            timezone_utc: None,
            flag_img_url: None,
            flag_emoji: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProxyHealthCheckResult {
    pub status: &'static str,
    pub latency_ms: Option<i64>,
    pub last_error: Option<String>,
    pub geo: Option<ProxyGeoInfo>,
}

pub(crate) fn check_account_proxy<'a>(
    proxy_url: &str,
    cached_flag_lookup: impl Fn(&str) -> Option<String> + 'a,
) -> ProxyHealthCheckResult {
    check_account_proxy_with_options(
        proxy_url,
        DEFAULT_PROXY_TEST_TARGETS,
        false,
        &cached_flag_lookup,
    )
}

fn check_account_proxy_with_options<'a>(
    proxy_url: &str,
    targets: &[&str],
    accept_invalid_certs: bool,
    cached_flag_lookup: &(dyn Fn(&str) -> Option<String> + 'a),
) -> ProxyHealthCheckResult {
    let (client, context) =
        match crate::account::proxy_testing::client::build_blocking_proxy_test_client(
            proxy_url,
            crate::account::proxy_testing::client::ProxyTestRedirectPolicy::Limited(10),
            accept_invalid_certs,
        ) {
            Ok(result) => result,
            Err(err) => {
                return ProxyHealthCheckResult {
                    status: STATUS_INVALID_URL,
                    latency_ms: None,
                    last_error: Some(err.message),
                    geo: None,
                };
            }
        };

    let mut geo_error = match check_proxy_geo(&client, proxy_url, cached_flag_lookup) {
        Ok((geo, latency_ms)) => {
            return ProxyHealthCheckResult {
                status: STATUS_OK,
                latency_ms: Some(latency_ms),
                last_error: None,
                geo: Some(geo),
            };
        }
        Err(err) => Some(err),
    };

    let mut last_error = None;
    for target in targets
        .iter()
        .copied()
        .filter(|value| !value.trim().is_empty())
    {
        let started_at = Instant::now();
        match client.get(target).send() {
            Ok(response) if response.status().is_success() => {
                return ProxyHealthCheckResult {
                    status: STATUS_OK,
                    latency_ms: Some(started_at.elapsed().as_millis().min(i64::MAX as u128) as i64),
                    last_error: None,
                    geo: geo_error.take().map(ProxyGeoInfo::error),
                };
            }
            Ok(response) => {
                last_error = Some(format!(
                    "proxy test GET {target} returned HTTP {}",
                    response.status()
                ));
            }
            Err(err) => {
                let mapped = crate::account::proxy_testing::errors::map_proxy_test_reqwest_error(
                    "proxy test GET",
                    proxy_url,
                    err,
                );
                if looks_like_local_proxy_runtime_error(&context.parsed_proxy_url, &mapped) {
                    return ProxyHealthCheckResult {
                        status: STATUS_RUNTIME_ERROR,
                        latency_ms: None,
                        last_error: Some(format!(
                            "local proxy runtime unavailable: {}",
                            mapped.message
                        )),
                        geo: geo_error.take().map(ProxyGeoInfo::error),
                    };
                }
                last_error = Some(format!(
                    "proxy test GET {target} failed [{}]: {}",
                    mapped.code.as_str(),
                    mapped.message
                ));
            }
        }
    }

    ProxyHealthCheckResult {
        status: STATUS_FAILED,
        latency_ms: None,
        last_error: Some(
            last_error.unwrap_or_else(|| "proxy test did not have any target URLs".to_string()),
        ),
        geo: geo_error.take().map(ProxyGeoInfo::error),
    }
}

fn proxy_geo_provider() -> String {
    std::env::var(ENV_PROXY_GEO_PROVIDER)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PROXY_GEO_PROVIDER.to_string())
}

fn proxy_geo_endpoint() -> String {
    #[cfg(test)]
    if let Ok(url) = std::env::var("TEST_PROXY_GEO_ENDPOINT") {
        return url;
    }

    std::env::var(ENV_PROXY_GEO_ENDPOINT)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| IPWHOIS_ENDPOINT.to_string())
}

fn check_proxy_geo<'a>(
    client: &Client,
    proxy_url: &str,
    cached_flag_lookup: &(dyn Fn(&str) -> Option<String> + 'a),
) -> Result<(ProxyGeoInfo, i64), String> {
    match proxy_geo_provider().as_str() {
        "ipwhois" => check_ipwhois_geo(client, proxy_url, cached_flag_lookup),
        other => Err(format!("unsupported proxy geo provider: {other}")),
    }
}

fn check_ipwhois_geo<'a>(
    client: &Client,
    proxy_url: &str,
    cached_flag_lookup: &(dyn Fn(&str) -> Option<String> + 'a),
) -> Result<(ProxyGeoInfo, i64), String> {
    let started_at = Instant::now();
    let response = client.get(proxy_geo_endpoint()).send().map_err(|err| {
        let mapped = crate::account::proxy_testing::errors::map_proxy_test_reqwest_error(
            "ipwho.is request",
            proxy_url,
            err,
        );
        format!("[{}] {}", mapped.code.as_str(), mapped.message)
    })?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("ipwho.is returned HTTP {status}"));
    }

    let payload = response
        .json::<IpWhoIsResponse>()
        .map_err(|err| format!("parse ipwho.is response failed: {err}"))?;

    let latency_ms = started_at.elapsed().as_millis().min(i64::MAX as u128) as i64;

    if !payload.success {
        return Err(payload
            .message
            .and_then(|message| normalize_optional_text(Some(message)))
            .unwrap_or_else(|| "ipwho.is returned success=false".to_string()));
    }

    let ip = normalize_optional_text(payload.ip);
    if ip.is_none() {
        return Err("ipwho.is response did not contain ip".to_string());
    }

    let connection = payload.connection.as_ref();
    let timezone = payload.timezone.as_ref();
    let flag = payload.flag.as_ref();

    let country_code =
        normalize_optional_text(payload.country_code).map(|value| value.to_ascii_uppercase());

    let flag_img_url = if let Some(code) = &country_code {
        if let Some(cached) = cached_flag_lookup(code) {
            Some(cached)
        } else if let Some(url) = flag.and_then(|f| normalize_optional_text(f.img.clone())) {
            match client.get(&url).send() {
                Ok(res) if res.status().is_success() => {
                    if let Ok(bytes) = res.bytes() {
                        use base64::{engine::general_purpose, Engine as _};
                        let b64 = general_purpose::STANDARD.encode(&bytes);
                        Some(format!("data:image/svg+xml;base64,{}", b64))
                    } else {
                        Some(url)
                    }
                }
                _ => Some(url),
            }
        } else {
            None
        }
    } else {
        flag.and_then(|f| normalize_optional_text(f.img.clone()))
    };

    Ok((
        ProxyGeoInfo {
            ip,
            country_code,
            country_name: normalize_optional_text(payload.country),
            region_name: normalize_optional_text(payload.region),
            city_name: normalize_optional_text(payload.city),
            geo_checked_at: Some(codexmanager_core::storage::now_ts()),
            geo_error: None,

            asn: connection.and_then(|c| c.asn),
            as_org: connection.and_then(|c| normalize_optional_text(c.org.clone())),
            isp: connection.and_then(|c| normalize_optional_text(c.isp.clone())),
            as_domain: connection.and_then(|c| normalize_optional_text(c.domain.clone())),

            timezone_id: timezone.and_then(|tz| normalize_optional_text(tz.id.clone())),
            timezone_offset: timezone.and_then(|tz| tz.offset),
            timezone_utc: timezone.and_then(|tz| normalize_optional_text(tz.utc.clone())),

            flag_img_url,
            flag_emoji: flag.and_then(|f| normalize_optional_text(f.emoji.clone())),
        },
        latency_ms,
    ))
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn looks_like_local_proxy_runtime_error(
    proxy_url: &Url,
    error: &crate::account::proxy_testing::errors::ProxyTestError,
) -> bool {
    if !is_loopback_proxy_url(proxy_url) {
        return false;
    }
    match error.code {
        crate::account::proxy_testing::errors::ProxyTestErrorCode::ProxyConnectTimeout
        | crate::account::proxy_testing::errors::ProxyTestErrorCode::ProxyConnectFailed
        | crate::account::proxy_testing::errors::ProxyTestErrorCode::ProxyTunnelFailed => {
            return true;
        }
        _ => {}
    }
    let message = error.message.to_ascii_lowercase();
    message.contains("connection refused")
        || message.contains("unsuccessful tunnel")
        || message.contains("tcp connect error")
        || message.contains("error trying to connect")
        || message.contains("proxy connect")
        || message.contains("channel closed")
}

fn is_loopback_proxy_url(proxy_url: &Url) -> bool {
    match proxy_url.host() {
        Some(Host::Ipv4(addr)) => addr.is_loopback(),
        Some(Host::Ipv6(addr)) => addr.is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::STATUS_RUNTIME_ERROR;

    #[test]
    fn proxy_health_check_marks_loopback_connect_refused_as_runtime_error() {
        let free_port = reserve_free_port();
        let proxy_url = format!("http://127.0.0.1:{free_port}");

        let result = super::check_account_proxy_with_options(
            &proxy_url,
            &["https://www.gstatic.com/generate_204"],
            true,
            &|_| None,
        );

        assert_eq!(result.status, STATUS_RUNTIME_ERROR);
        assert_eq!(result.latency_ms, None);
        let error = result.last_error.as_deref().expect("last_error");
        assert!(error.contains("local proxy runtime unavailable"));
    }

    fn reserve_free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind free port probe")
            .local_addr()
            .expect("free port addr")
            .port()
    }
}
