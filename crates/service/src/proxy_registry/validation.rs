pub(crate) fn normalize_proxy_profile_url(proxy_url: &str) -> Result<String, String> {
    crate::account_proxy::normalize_supported_proxy_url(proxy_url)
}
