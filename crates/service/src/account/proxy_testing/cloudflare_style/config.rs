use serde::{Deserialize, Serialize};

/// Cloudflare endpoint payload sizes (matching cfspeedtest defaults).
pub(crate) const CF_PAYLOADS: &[u64] = &[
    100_000,     // 100 KB
    1_000_000,   // 1 MB
    10_000_000,  // 10 MB
    25_000_000,  // 25 MB
    50_000_000,  // 50 MB
    100_000_000, // 100 MB
];

/// Dynamic stop: skip larger payloads if average throughput drops below this.
pub(crate) const DYNAMIC_STOP_THRESHOLD_MBPS: f64 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CfPresetValue {
    All,
    Size100Kb,
    Size1Mb,
    Size10Mb,
    Size25Mb,
    Size50Mb,
}

impl CfPresetValue {
    pub fn from_str(s: &str) -> Self {
        match s {
            "100kb" => Self::Size100Kb,
            "1mb" => Self::Size1Mb,
            "10mb" => Self::Size10Mb,
            "25mb" => Self::Size25Mb,
            "50mb" => Self::Size50Mb,
            _ => Self::All,
        }
    }

    pub fn max_bytes(self, is_upload: bool) -> u64 {
        match self {
            Self::Size100Kb => 100_000,
            Self::Size1Mb => 1_000_000,
            Self::Size10Mb => 10_000_000,
            Self::Size25Mb => 25_000_000,
            Self::Size50Mb => 50_000_000,
            Self::All => {
                if is_upload {
                    50_000_000
                } else {
                    25_000_000
                }
            }
        }
    }

    pub fn is_all(self) -> bool {
        matches!(self, Self::All)
    }

    pub fn latency_samples(self) -> usize {
        match self {
            Self::Size100Kb => 3,
            Self::Size1Mb => 5,
            _ => 10,
        }
    }
}

/// Runtime configuration for a cloudflare-style speed test.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfStyleConfig {
    #[serde(default)]
    pub download_preset: Option<String>,
    #[serde(default)]
    pub upload_preset: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub run_upload: Option<bool>,
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

fn default_timeout_secs() -> u64 {
    120
}

fn default_base_url() -> String {
    "https://speed.cloudflare.com".to_string()
}

impl Default for CfStyleConfig {
    fn default() -> Self {
        Self {
            download_preset: None,
            upload_preset: None,
            timeout_secs: default_timeout_secs(),
            run_upload: None,
            base_url: default_base_url(),
        }
    }
}

impl CfStyleConfig {
    pub fn should_run_upload(&self) -> bool {
        self.run_upload.unwrap_or(true)
    }

    pub fn get_download_preset(&self) -> CfPresetValue {
        if let Some(ref dp) = self.download_preset {
            CfPresetValue::from_str(dp)
        } else {
            CfPresetValue::All
        }
    }

    pub fn get_upload_preset(&self) -> CfPresetValue {
        if let Some(ref up) = self.upload_preset {
            CfPresetValue::from_str(up)
        } else {
            CfPresetValue::All
        }
    }
}
