use codexmanager_core::rpc::types::{
    ProxyTestDefaults, ProxyTestFileSizePreset, ProxyTestPresetsResult,
    ProxyTestProviderFilePreset, ProxyTestSpeedProviderPreset, ProxyTestUploadEndpointStatus,
};

const ENV_PROXY_TEST_UPLOAD_URL: &str = "CODEXMANAGER_PROXY_TEST_UPLOAD_URL";
const APP_SETTING_PROXY_TEST_UPLOAD_URL_KEY: &str = "proxy_test.upload_url";

const DEFAULT_SPEED_PROVIDER_ID: &str = "cloudflare_http_rust";
const DEFAULT_FILE_SIZE_ID: &str = "size_25mb";
const DEFAULT_LATENCY_PRESET_ID: &str = "google_gstatic_204";

const SIZE_100KB: &str = "size_100kb";
const SIZE_1MB: &str = "size_1mb";
const SIZE_10MB: &str = "size_10mb";
const SIZE_25MB: &str = "size_25mb";
const SIZE_100MB: &str = "size_100mb";
const SIZE_500MB_CAP: &str = "size_500mb_cap";
const SIZE_1GB: &str = "size_1gb";
const SIZE_10GB: &str = "size_10gb";

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct ResolvedDownloadTestTarget {
    pub provider_id: String,
    pub provider_family: String,
    pub file_size_id: String,
    pub size_bytes: i64,
    pub download_url: String,
    pub read_limit_bytes: Option<i64>,
}

pub(crate) fn proxy_test_presets() -> ProxyTestPresetsResult {
    ProxyTestPresetsResult {
        speed_providers: speed_providers(),
        file_sizes: file_sizes(),
        defaults: ProxyTestDefaults {
            speed_provider_id: DEFAULT_SPEED_PROVIDER_ID.to_string(),
            file_size_id: DEFAULT_FILE_SIZE_ID.to_string(),
            latency_preset_id: DEFAULT_LATENCY_PRESET_ID.to_string(),
        },
        upload_endpoint: upload_endpoint_status(),
    }
}

#[allow(dead_code)]
pub(crate) fn resolve_download_test_target(
    provider_id: Option<&str>,
    file_size_id: Option<&str>,
) -> Result<ResolvedDownloadTestTarget, String> {
    let requested_provider_id = provider_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_SPEED_PROVIDER_ID);
    let requested_file_size_id = file_size_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_FILE_SIZE_ID);

    let provider = speed_providers()
        .into_iter()
        .find(|provider| provider.id == requested_provider_id)
        .ok_or_else(|| format!("unknown speed provider: {requested_provider_id}"))?;
    let file_size = file_sizes()
        .into_iter()
        .find(|size| size.id == requested_file_size_id)
        .ok_or_else(|| format!("unknown file size: {requested_file_size_id}"))?;
    let provider_file = provider
        .files
        .iter()
        .find(|file| file.file_size_id == requested_file_size_id)
        .ok_or_else(|| {
            format!(
                "speed provider {} does not support file size {}",
                provider.id, requested_file_size_id
            )
        })?;

    Ok(ResolvedDownloadTestTarget {
        provider_id: provider.id,
        provider_family: provider.provider_family,
        file_size_id: file_size.id,
        size_bytes: file_size.bytes,
        download_url: provider_file.download_url.clone(),
        read_limit_bytes: provider_file.read_limit_bytes,
    })
}



fn file_sizes() -> Vec<ProxyTestFileSizePreset> {
    vec![
        ProxyTestFileSizePreset {
            id: SIZE_100KB.to_string(),
            label: "100KB".to_string(),
            bytes: 100_000,
            warning: false,
        },
        ProxyTestFileSizePreset {
            id: SIZE_1MB.to_string(),
            label: "1MB".to_string(),
            bytes: 1_000_000,
            warning: false,
        },
        ProxyTestFileSizePreset {
            id: SIZE_10MB.to_string(),
            label: "10MB".to_string(),
            bytes: 10_000_000,
            warning: false,
        },
        ProxyTestFileSizePreset {
            id: SIZE_25MB.to_string(),
            label: "25MB".to_string(),
            bytes: 25_000_000,
            warning: false,
        },
        ProxyTestFileSizePreset {
            id: SIZE_100MB.to_string(),
            label: "100MB".to_string(),
            bytes: 100_000_000,
            warning: false,
        },
        ProxyTestFileSizePreset {
            id: SIZE_500MB_CAP.to_string(),
            label: "500MB cap".to_string(),
            bytes: 500_000_000,
            warning: false,
        },
        ProxyTestFileSizePreset {
            id: SIZE_1GB.to_string(),
            label: "1GB".to_string(),
            bytes: 1_000_000_000,
            warning: true,
        },
        ProxyTestFileSizePreset {
            id: SIZE_10GB.to_string(),
            label: "10GB".to_string(),
            bytes: 10_000_000_000,
            warning: true,
        },
    ]
}

fn speed_providers() -> Vec<ProxyTestSpeedProviderPreset> {
    vec![
        ProxyTestSpeedProviderPreset {
            id: "cloudflare_http_rust".to_string(),
            label: "Cloudflare HTTP".to_string(),
            provider_family: "cloudflare_http_rust".to_string(),
            files: vec![
                provider_file(SIZE_100KB, "https://speed.cloudflare.com/__down?bytes=100000"),
                provider_file(SIZE_1MB, "https://speed.cloudflare.com/__down?bytes=1000000"),
                provider_file(SIZE_10MB, "https://speed.cloudflare.com/__down?bytes=10000000"),
                provider_file(SIZE_25MB, "https://speed.cloudflare.com/__down?bytes=25000000"),
                provider_file(SIZE_100MB, "https://speed.cloudflare.com/__down?bytes=100000000"),
            ],
        },
        ProxyTestSpeedProviderPreset {
            id: "cachefly".to_string(),
            label: "CacheFly".to_string(),
            provider_family: "cachefly".to_string(),
            files: vec![
                provider_file(SIZE_1MB, "http://cachefly.cachefly.net/1mb.test"),
                provider_file(SIZE_10MB, "http://cachefly.cachefly.net/10mb.test"),
                provider_file(SIZE_100MB, "http://cachefly.cachefly.net/100mb.test"),
            ],
        },
        hetzner_provider(
            "hetzner_fsn1",
            "Hetzner FSN1",
            "https://fsn1-speed.hetzner.com",
        ),
        hetzner_provider(
            "hetzner_nbg1",
            "Hetzner NBG1",
            "https://nbg1-speed.hetzner.com",
        ),
        hetzner_provider(
            "hetzner_hel1",
            "Hetzner HEL1",
            "https://hel1-speed.hetzner.com",
        ),
        hetzner_provider(
            "hetzner_ash",
            "Hetzner ASH",
            "https://ash-speed.hetzner.com",
        ),
        hetzner_provider(
            "hetzner_hil",
            "Hetzner HIL",
            "https://hil-speed.hetzner.com",
        ),
        hetzner_provider(
            "hetzner_sin",
            "Hetzner SIN",
            "https://sin-speed.hetzner.com",
        ),
        ovh_provider("ovh_gra", "OVH GRA", "https://gra.proof.ovh.net/files"),
        ovh_provider("ovh_rbx", "OVH RBX", "https://rbx.proof.ovh.net/files"),
        ovh_provider("ovh_sbg", "OVH SBG", "https://sbg.proof.ovh.net/files"),
        ovh_provider("ovh_eri", "OVH ERI", "https://eri.proof.ovh.net/files"),
        ovh_provider("ovh_vin", "OVH VIN", "https://vin.proof.ovh.us/files"),
        ovh_provider("ovh_hil", "OVH HIL", "https://hil.proof.ovh.us/files"),
        ovh_provider("ovh_sgp", "OVH SGP", "https://sgp.proof.ovh.net/files"),
        ovh_provider("ovh_bom", "OVH BOM", "https://bom.proof.ovh.net/files"),
        ovh_provider("ovh_syd", "OVH SYD", "https://syd.proof.ovh.net/files"),
    ]
}

fn provider_file(file_size_id: &str, download_url: &str) -> ProxyTestProviderFilePreset {
    ProxyTestProviderFilePreset {
        file_size_id: file_size_id.to_string(),
        download_url: download_url.to_string(),
        read_limit_bytes: None,
    }
}

fn capped_provider_file(
    file_size_id: &str,
    download_url: &str,
    read_limit_bytes: i64,
) -> ProxyTestProviderFilePreset {
    ProxyTestProviderFilePreset {
        file_size_id: file_size_id.to_string(),
        download_url: download_url.to_string(),
        read_limit_bytes: Some(read_limit_bytes),
    }
}

fn hetzner_provider(id: &str, label: &str, base_url: &str) -> ProxyTestSpeedProviderPreset {
    ProxyTestSpeedProviderPreset {
        id: id.to_string(),
        label: label.to_string(),
        provider_family: "hetzner".to_string(),
        files: vec![
            provider_file(SIZE_100MB, &format!("{base_url}/100MB.bin")),
            capped_provider_file(SIZE_500MB_CAP, &format!("{base_url}/1GB.bin"), 500_000_000),
            provider_file(SIZE_1GB, &format!("{base_url}/1GB.bin")),
            provider_file(SIZE_10GB, &format!("{base_url}/10GB.bin")),
        ],
    }
}

fn ovh_provider(id: &str, label: &str, base_url: &str) -> ProxyTestSpeedProviderPreset {
    ProxyTestSpeedProviderPreset {
        id: id.to_string(),
        label: label.to_string(),
        provider_family: "ovh".to_string(),
        files: vec![
            provider_file(SIZE_1MB, &format!("{base_url}/1Mb.dat")),
            provider_file(SIZE_10MB, &format!("{base_url}/10Mb.dat")),
            provider_file(SIZE_100MB, &format!("{base_url}/100Mb.dat")),
            capped_provider_file(SIZE_500MB_CAP, &format!("{base_url}/1Gb.dat"), 500_000_000),
            provider_file(SIZE_1GB, &format!("{base_url}/1Gb.dat")),
            provider_file(SIZE_10GB, &format!("{base_url}/10Gb.dat")),
        ],
    }
}

pub(crate) fn upload_endpoint_status() -> ProxyTestUploadEndpointStatus {
    if let Some(url) =
        crate::app_settings::get_persisted_app_setting(APP_SETTING_PROXY_TEST_UPLOAD_URL_KEY)
    {
        return ProxyTestUploadEndpointStatus {
            status: "configured".to_string(),
            configured: true,
            source: "app_setting".to_string(),
            url: Some(url),
        };
    }

    if let Some(url) = std::env::var(ENV_PROXY_TEST_UPLOAD_URL)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return ProxyTestUploadEndpointStatus {
            status: "configured".to_string(),
            configured: true,
            source: "env".to_string(),
            url: Some(url),
        };
    }

    ProxyTestUploadEndpointStatus {
        status: "not_configured".to_string(),
        configured: false,
        source: "none".to_string(),
        url: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        proxy_test_presets, resolve_download_test_target,
        upload_endpoint_status, APP_SETTING_PROXY_TEST_UPLOAD_URL_KEY, DEFAULT_FILE_SIZE_ID,
        DEFAULT_SPEED_PROVIDER_ID, ENV_PROXY_TEST_UPLOAD_URL,
        SIZE_500MB_CAP,
    };

    #[test]
    fn presets_default_to_cloudflare_25mb() {
        let presets = proxy_test_presets();

        assert_eq!(
            presets.defaults.speed_provider_id,
            DEFAULT_SPEED_PROVIDER_ID
        );
        assert_eq!(presets.defaults.file_size_id, DEFAULT_FILE_SIZE_ID);
    }

    #[test]
    fn provider_size_mapping_matches_plan() {
        let presets = proxy_test_presets();
        let cachefly = presets
            .speed_providers
            .iter()
            .find(|provider| provider.id == "cachefly")
            .expect("cachefly provider");
        assert_eq!(cachefly.files.len(), 3);
        assert!(!cachefly
            .files
            .iter()
            .any(|entry| entry.file_size_id == SIZE_500MB_CAP));

        let hetzner = presets
            .speed_providers
            .iter()
            .find(|provider| provider.id == "hetzner_fsn1")
            .expect("hetzner provider");
        let capped = hetzner
            .files
            .iter()
            .find(|entry| entry.file_size_id == SIZE_500MB_CAP)
            .expect("hetzner 500mb cap");
        assert_eq!(capped.read_limit_bytes, Some(500_000_000));
        assert!(capped.download_url.ends_with("/1GB.bin"));

        let ovh = presets
            .speed_providers
            .iter()
            .find(|provider| provider.id == "ovh_gra")
            .expect("ovh provider");
        let capped = ovh
            .files
            .iter()
            .find(|entry| entry.file_size_id == SIZE_500MB_CAP)
            .expect("ovh 500mb cap");
        assert_eq!(capped.read_limit_bytes, Some(500_000_000));
        assert!(capped.download_url.ends_with("/1Gb.dat"));
    }

    #[test]
    fn resolve_download_test_target_defaults_to_cloudflare_25mb() {
        let target = resolve_download_test_target(None, None).expect("resolve default target");
        assert_eq!(target.provider_id, DEFAULT_SPEED_PROVIDER_ID);
        assert_eq!(target.file_size_id, DEFAULT_FILE_SIZE_ID);
        assert_eq!(target.size_bytes, 25_000_000);
        assert_eq!(
            target.download_url,
            "https://speed.cloudflare.com/__down?bytes=25000000"
        );
        assert_eq!(target.read_limit_bytes, None);
    }

    #[test]
    fn resolve_download_test_target_uses_read_limit_for_500mb_cap() {
        let target = resolve_download_test_target(Some("hetzner_fsn1"), Some(SIZE_500MB_CAP))
            .expect("resolve 500mb cap target");
        assert_eq!(target.file_size_id, SIZE_500MB_CAP);
        assert_eq!(target.read_limit_bytes, Some(500_000_000));
        assert_eq!(
            target.download_url,
            "https://fsn1-speed.hetzner.com/1GB.bin"
        );
    }

    #[test]
    fn resolve_download_test_target_rejects_invalid_provider_size_combo() {
        let err = resolve_download_test_target(Some("hetzner_fsn1"), Some("size_10mb"))
            .expect_err("invalid provider/size combo should fail");
        assert!(err.contains("does not support file size"));
    }

    #[test]
    fn upload_endpoint_status_uses_env_when_present() {
        let _guard = crate::test_env_guard();
        let original_db_path = std::env::var_os("CODEXMANAGER_DB_PATH");
        let db_path = std::env::temp_dir().join(format!(
            "codexmanager-proxy-test-presets-{}-{}.sqlite",
            std::process::id(),
            codexmanager_core::storage::now_ts()
        ));
        std::env::set_var("CODEXMANAGER_DB_PATH", &db_path);
        std::env::remove_var(ENV_PROXY_TEST_UPLOAD_URL);
        let _ = crate::app_settings::save_persisted_app_setting(
            APP_SETTING_PROXY_TEST_UPLOAD_URL_KEY,
            None,
        );

        std::env::set_var(
            ENV_PROXY_TEST_UPLOAD_URL,
            "https://upload.example.com/proxy-test",
        );
        let status = upload_endpoint_status();
        assert!(status.configured);
        assert_eq!(status.source, "env");
        assert_eq!(
            status.url.as_deref(),
            Some("https://upload.example.com/proxy-test")
        );
        std::env::remove_var(ENV_PROXY_TEST_UPLOAD_URL);
        if let Some(value) = original_db_path {
            std::env::set_var("CODEXMANAGER_DB_PATH", value);
        } else {
            std::env::remove_var("CODEXMANAGER_DB_PATH");
        }
        let _ = std::fs::remove_file(db_path);
    }

}

