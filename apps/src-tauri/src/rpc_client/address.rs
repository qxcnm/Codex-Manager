use std::net::{SocketAddr, ToSocketAddrs};

pub(crate) fn normalize_addr(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("addr is empty".to_string());
    }
    let mut value = trimmed;
    if let Some(rest) = value.strip_prefix("http://") {
        value = rest;
    }
    if let Some(rest) = value.strip_prefix("https://") {
        value = rest;
    }
    let value = value.split('/').next().unwrap_or(value);
    if value.contains(':') {
        Ok(normalize_host(value))
    } else {
        Ok(format!("localhost:{value}"))
    }
}

pub(crate) fn resolve_service_addr(addr: Option<String>) -> Result<String, String> {
    if let Some(addr) = addr {
        return normalize_addr(&addr);
    }
    if let Ok(env_addr) = std::env::var("CODEXMANAGER_SERVICE_ADDR") {
        if let Ok(addr) = normalize_addr(&env_addr) {
            return Ok(addr);
        }
    }
    Ok(codexmanager_service::DEFAULT_ADDR.to_string())
}

pub(crate) fn resolve_socket_addrs(addr: &str) -> Result<Vec<SocketAddr>, String> {
    let addrs = addr
        .to_socket_addrs()
        .map_err(|err| format!("Invalid service address {addr}: {err}"))?;
    let mut out = Vec::new();
    for sock in addrs {
        if !out.iter().any(|item| item == &sock) {
            out.push(sock);
        }
    }
    if out.is_empty() {
        return Err(format!(
            "Invalid service address {addr}: no address resolved"
        ));
    }
    Ok(out)
}

fn normalize_host(value: &str) -> String {
    if let Some((host, port)) = value.rsplit_once(':') {
        let mapped = match host {
            "127.0.0.1" | "0.0.0.0" | "::1" | "[::1]" => "localhost",
            _ => host,
        };
        format!("{mapped}:{port}")
    } else {
        value.to_string()
    }
}
