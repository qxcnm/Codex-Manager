use reqwest::blocking::Client;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

use super::apply::resolve_portable_restart_exe;
use super::github::fetch_latest_release;
use super::model::{GitHubAsset, PendingUpdate, UpdateCheckResponse, UpdatePrepareResponse};
use super::runtime::{
    current_exe_path, current_mode_and_marker, http_client, normalize_version, now_unix_secs,
    resolve_update_repo, should_include_prerelease_updates, USER_AGENT,
};
use super::state::{set_last_check, updates_root_dir, write_pending_update};

pub(super) struct ResolvedUpdateContext {
    pub(super) check: UpdateCheckResponse,
    payload_asset: Option<GitHubAsset>,
}

/// 函数 `append_prepare_log`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - log_path: 参数 log_path
/// - message: 参数 message
///
/// # 返回
/// 无
fn append_prepare_log(log_path: &Path, message: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let line = format!("[{timestamp}] {message}\n");

    if let Some(parent) = log_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            log::warn!(
                "event=update_log_directory_create_failed phase=prepare path={} error={err}",
                parent.display()
            );
            return;
        }
    }
    match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        Ok(mut file) => {
            if let Err(err) = file.write_all(line.as_bytes()).and_then(|()| file.flush()) {
                log::warn!(
                    "event=update_log_write_failed phase=prepare path={} error={err}",
                    log_path.display()
                );
            }
        }
        Err(err) => log::warn!(
            "event=update_log_open_failed phase=prepare path={} error={err}",
            log_path.display()
        ),
    }
}

/// 函数 `portable_asset_names_for_platform`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn portable_asset_names_for_platform(latest_version: &str) -> Vec<String> {
    let v = latest_version.trim().trim_start_matches(['v', 'V']);
    if cfg!(target_os = "windows") {
        vec![
            "CodexManager-portable.exe".to_string(),
            format!("CodexManager-{v}-windows-portable.zip"),
            "CodexManager-windows-portable.zip".to_string(),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            format!("CodexManager-{v}-macos-portable.zip"),
            "CodexManager-macos-portable.zip".to_string(),
        ]
    } else {
        vec![
            format!("CodexManager-{v}-linux-portable.zip"),
            "CodexManager-linux-portable.zip".to_string(),
        ]
    }
}

/// 函数 `macos_current_arch_tokens`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn macos_current_arch_tokens() -> &'static [&'static str] {
    if cfg!(target_arch = "aarch64") {
        &["aarch64", "arm64"]
    } else if cfg!(target_arch = "x86_64") {
        &["x64", "x86_64"]
    } else {
        &[]
    }
}

/// 函数 `is_dmg_asset`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn is_dmg_asset(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".dmg")
}

/// 函数 `dmg_name_has_arch_suffix`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - suffix: 参数 suffix
///
/// # 返回
/// 返回函数执行结果
fn dmg_name_has_arch_suffix(name: &str, suffix: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let suffix = suffix.to_ascii_lowercase();
    lower.ends_with(&format!("_{suffix}.dmg"))
        || lower.ends_with(&format!("-{suffix}.dmg"))
        || lower.ends_with(&format!(".{suffix}.dmg"))
}

/// 函数 `select_macos_dmg_asset_for_arch`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - assets: 参数 assets
/// - arch_tokens: 参数 arch_tokens
///
/// # 返回
/// 返回函数执行结果
fn select_macos_dmg_asset_for_arch(
    assets: &[GitHubAsset],
    arch_tokens: &[&str],
) -> Option<GitHubAsset> {
    let dmg_assets = assets
        .iter()
        .filter(|asset| is_dmg_asset(&asset.name))
        .cloned()
        .collect::<Vec<_>>();
    if dmg_assets.is_empty() {
        return None;
    }

    for arch in arch_tokens {
        if let Some(asset) = dmg_assets
            .iter()
            .find(|asset| dmg_name_has_arch_suffix(&asset.name, arch))
        {
            return Some(asset.clone());
        }
    }

    for universal in ["universal", "universal2"] {
        if let Some(asset) = dmg_assets
            .iter()
            .find(|asset| dmg_name_has_arch_suffix(&asset.name, universal))
        {
            return Some(asset.clone());
        }
    }

    let known_arch_suffixes = [
        "aarch64",
        "arm64",
        "x64",
        "x86_64",
        "universal",
        "universal2",
    ];
    dmg_assets.into_iter().find(|asset| {
        !known_arch_suffixes
            .iter()
            .any(|suffix| dmg_name_has_arch_suffix(&asset.name, suffix))
    })
}

/// 函数 `select_payload_asset`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - mode: 参数 mode
/// - latest_version: 参数 latest_version
/// - assets: 参数 assets
///
/// # 返回
/// 返回函数执行结果
fn select_payload_asset(
    mode: &str,
    latest_version: &str,
    assets: &[GitHubAsset],
) -> Option<GitHubAsset> {
    if mode == "portable" {
        let portable_names = portable_asset_names_for_platform(latest_version);
        for expected in portable_names {
            if let Some(asset) = assets
                .iter()
                .find(|asset| asset.name.eq_ignore_ascii_case(&expected))
            {
                return Some(asset.clone());
            }
        }
        return None;
    }

    if cfg!(target_os = "windows") {
        if let Some(exe) = assets.iter().find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.ends_with(".exe") && !name.contains("portable")
        }) {
            return Some(exe.clone());
        }
        return assets
            .iter()
            .find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name.ends_with(".msi") && !name.contains("portable")
            })
            .cloned();
    }

    if cfg!(target_os = "macos") {
        return select_macos_dmg_asset_for_arch(assets, macos_current_arch_tokens());
    }

    if let Some(appimage) = assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(".appimage"))
    {
        return Some(appimage.clone());
    }
    if let Some(deb) = assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(".deb"))
    {
        return Some(deb.clone());
    }
    assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(".rpm"))
        .cloned()
}

/// 函数 `sanitize_tag`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - tag: 参数 tag
///
/// # 返回
/// 返回函数执行结果
fn sanitize_tag(tag: &str) -> String {
    let out: String = tag
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

/// 函数 `download_to_file`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - client: 参数 client
/// - url: 参数 url
/// - target: 参数 target
///
/// # 返回
/// 返回函数执行结果
fn download_to_file(client: &Client, url: &str, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建下载目录失败：{err}"))?;
    }
    let mut resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .map_err(|err| format!("发起下载请求失败：{err}"))?
        .error_for_status()
        .map_err(|err| format!("下载响应异常：{err}"))?;

    let mut file = File::create(target).map_err(|err| format!("创建文件失败：{err}"))?;
    std::io::copy(&mut resp, &mut file).map_err(|err| format!("写入文件失败：{err}"))?;
    file.flush()
        .map_err(|err| format!("刷新文件缓冲区失败：{err}"))
}

/// 函数 `extract_zip_archive`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - zip_path: 参数 zip_path
/// - target_dir: 参数 target_dir
///
/// # 返回
/// 返回函数执行结果
fn extract_zip_archive(zip_path: &Path, target_dir: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|err| format!("打开 ZIP 包失败：{err}"))?;
    let mut archive = ZipArchive::new(file).map_err(|err| format!("读取 ZIP 包失败：{err}"))?;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .map_err(|err| format!("读取 ZIP 条目失败：{err}"))?;
        let Some(relative_path) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let out_path = target_dir.join(relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|err| format!("创建目录失败：{err}"))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("创建父目录失败：{err}"))?;
        }
        let mut out_file = File::create(&out_path).map_err(|err| format!("创建文件失败：{err}"))?;
        std::io::copy(&mut entry, &mut out_file).map_err(|err| format!("解压文件失败：{err}"))?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
        }
    }

    Ok(())
}

/// 函数 `stage_portable_payload`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - payload_path: 参数 payload_path
/// - payload_name: 参数 payload_name
/// - staging_dir: 参数 staging_dir
///
/// # 返回
/// 返回函数执行结果
fn stage_portable_payload(
    payload_path: &Path,
    payload_name: &str,
    staging_dir: &Path,
) -> Result<(), String> {
    let extension = payload_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if extension == "zip" {
        return extract_zip_archive(payload_path, staging_dir);
    }

    let file_name = Path::new(payload_name)
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("无法解析便携更新文件名：{payload_name}"))?;
    let target_path = staging_dir.join(file_name);
    fs::copy(payload_path, &target_path).map_err(|err| format!("复制便携更新文件失败：{err}"))?;

    #[cfg(unix)]
    {
        let _ = fs::set_permissions(&target_path, fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

/// 函数 `detach_macos_dmg_mount`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - mount_dir: 参数 mount_dir
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "macos")]
fn detach_macos_dmg_mount(mount_dir: &Path) -> Result<(), String> {
    let status = Command::new("hdiutil")
        .arg("detach")
        .arg(mount_dir)
        .arg("-force")
        .status()
        .map_err(|err| format!("卸载 dmg 挂载点失败：{err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("卸载 dmg 挂载点失败，退出码：{status}"))
    }
}

/// 函数 `find_first_app_bundle`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - root: 参数 root
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "macos")]
fn find_first_app_bundle(root: &Path) -> Result<PathBuf, String> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)
            .map_err(|err| format!("读取 app 包目录失败 {}：{err}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| format!("读取 app 包目录项失败：{err}"))?;
            let path = entry.path();
            if path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("app"))
                .unwrap_or(false)
                && path.is_dir()
            {
                return Ok(path);
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }

    Err(format!(
        "在 dmg 挂载目录中未找到 .app 包：{}",
        root.display()
    ))
}

/// 函数 `copy_macos_app_bundle`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - source: 参数 source
/// - target: 参数 target
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "macos")]
fn copy_macos_app_bundle(source: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        fs::remove_dir_all(target)
            .map_err(|err| format!("清理暂存 app 包失败 {}：{err}", target.display()))?;
    }
    let status = Command::new("ditto")
        .arg(source)
        .arg(target)
        .status()
        .map_err(|err| format!("复制 app 包失败：{err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "复制 app 包失败：{} -> {}，退出码：{status}",
            source.display(),
            target.display()
        ))
    }
}

/// 函数 `stage_macos_dmg_payload`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - payload_path: 参数 payload_path
/// - staging_dir: 参数 staging_dir
/// - log_path: 参数 log_path
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "macos")]
fn stage_macos_dmg_payload(
    payload_path: &Path,
    staging_dir: &Path,
    log_path: &Path,
) -> Result<(), String> {
    append_prepare_log(
        log_path,
        &format!("开始挂载 dmg：{}", payload_path.display()),
    );
    let mount_dir = staging_dir.join("__dmg_mount");
    if mount_dir.exists() {
        fs::remove_dir_all(&mount_dir)
            .map_err(|err| format!("清理旧的 dmg 挂载目录失败：{err}"))?;
    }
    fs::create_dir_all(&mount_dir).map_err(|err| format!("创建 dmg 挂载目录失败：{err}"))?;

    let attach_status = Command::new("hdiutil")
        .arg("attach")
        .arg(payload_path)
        .arg("-nobrowse")
        .arg("-readonly")
        .arg("-mountpoint")
        .arg(&mount_dir)
        .status()
        .map_err(|err| format!("挂载 dmg 失败：{err}"))?;
    if !attach_status.success() {
        append_prepare_log(log_path, &format!("挂载 dmg 失败，退出码：{attach_status}"));
        let _ = fs::remove_dir_all(&mount_dir);
        return Err(format!("挂载 dmg 失败，退出码：{attach_status}"));
    }
    append_prepare_log(log_path, &format!("dmg 已挂载到：{}", mount_dir.display()));

    let stage_result = (|| {
        let app_bundle = find_first_app_bundle(&mount_dir)?;
        append_prepare_log(
            log_path,
            &format!("在 dmg 中发现 app 包：{}", app_bundle.display()),
        );
        let app_name = app_bundle
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("无法解析 app 包名称：{}", app_bundle.display()))?;
        let target_bundle = staging_dir.join(app_name);
        let copied = copy_macos_app_bundle(&app_bundle, &target_bundle);
        if copied.is_ok() {
            append_prepare_log(
                log_path,
                &format!("已复制 app 包到暂存目录：{}", target_bundle.display()),
            );
        }
        copied
    })();

    let detach_result = detach_macos_dmg_mount(&mount_dir);
    let cleanup_result =
        fs::remove_dir_all(&mount_dir).map_err(|err| format!("清理 dmg 挂载目录失败：{err}"));

    if detach_result.is_ok() {
        append_prepare_log(log_path, "dmg 挂载点已卸载");
    }
    if cleanup_result.is_ok() {
        append_prepare_log(log_path, "dmg 挂载临时目录已清理");
    }

    stage_result?;
    detach_result?;
    cleanup_result?;
    append_prepare_log(log_path, "macOS dmg 更新暂存完成");
    Ok(())
}

/// 函数 `resolve_update_context`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn resolve_update_context() -> Result<ResolvedUpdateContext, String> {
    let repo = resolve_update_repo();
    let (mode, is_portable, _, _) = current_mode_and_marker()?;
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let current_semver = normalize_version(&current_version)?;
    let include_prerelease = should_include_prerelease_updates(&current_semver);

    let client = http_client()?;
    let release = fetch_latest_release(&client, &repo, include_prerelease)?;
    let latest_semver = normalize_version(&release.tag_name)?;
    let has_update = latest_semver > current_semver;

    let payload_asset = select_payload_asset(&mode, &latest_semver.to_string(), &release.assets);
    let can_prepare = has_update && payload_asset.is_some();
    let fetched_by_fallback = release.assets.is_empty();

    let reason = if !has_update {
        Some("当前版本已是最新".to_string())
    } else if fetched_by_fallback {
        Some(
            "已在 GitHub Releases 页面发现新版本，但发布资产元数据不可用（可能是 GitHub API 速率限制或页面解析偏移）；可设置 CODEXMANAGER_GITHUB_TOKEN 提高一键更新稳定性".to_string(),
        )
    } else if payload_asset.is_none() {
        Some("未找到当前平台/运行模式对应的发布资产".to_string())
    } else {
        None
    };

    Ok(ResolvedUpdateContext {
        check: UpdateCheckResponse {
            repo,
            mode,
            is_portable,
            has_update,
            can_prepare,
            current_version,
            latest_version: latest_semver.to_string(),
            release_tag: release.tag_name,
            release_name: release.name,
            published_at: release.published_at,
            reason,
            checked_at_unix_secs: now_unix_secs(),
        },
        payload_asset,
    })
}

/// 函数 `prepare_update_impl`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn prepare_update_impl(app: &tauri::AppHandle) -> Result<UpdatePrepareResponse, String> {
    let context = resolve_update_context()?;
    set_last_check(context.check.clone());

    if !context.check.has_update {
        return Err("当前版本已是最新".to_string());
    }
    if !context.check.can_prepare {
        return Err(context
            .check
            .reason
            .clone()
            .unwrap_or_else(|| "更新尚未准备就绪".to_string()));
    }

    let client = http_client()?;
    let payload_asset = context
        .payload_asset
        .clone()
        .ok_or_else(|| "缺少可下载安装的发布资产".to_string())?;
    let release_dir = updates_root_dir(app)?.join(sanitize_tag(&context.check.release_tag));
    fs::create_dir_all(&release_dir).map_err(|err| format!("创建发布目录失败：{err}"))?;
    let logs_dir = release_dir.join("logs");
    let prepare_log_path = logs_dir.join("prepare-update.log");
    append_prepare_log(
        &prepare_log_path,
        &format!(
            "开始准备更新，tag={}, asset={}",
            context.check.release_tag, payload_asset.name
        ),
    );

    let payload_path = release_dir.join(&payload_asset.name);
    download_to_file(&client, &payload_asset.browser_download_url, &payload_path)?;
    append_prepare_log(
        &prepare_log_path,
        &format!("更新资产下载完成：{}", payload_path.display()),
    );

    let mut pending = PendingUpdate {
        mode: context.check.mode.clone(),
        is_portable: context.check.is_portable,
        release_tag: context.check.release_tag.clone(),
        latest_version: context.check.latest_version.clone(),
        asset_name: payload_asset.name.clone(),
        asset_path: payload_path.display().to_string(),
        installer_path: None,
        staging_dir: None,
        prepared_at_unix_secs: now_unix_secs(),
    };

    if context.check.mode == "portable" {
        let staging_dir = release_dir.join("staging");
        if staging_dir.is_dir() {
            fs::remove_dir_all(&staging_dir).map_err(|err| format!("清理暂存目录失败：{err}"))?;
        }
        fs::create_dir_all(&staging_dir).map_err(|err| format!("创建暂存目录失败：{err}"))?;
        stage_portable_payload(&payload_path, &payload_asset.name, &staging_dir)?;
        let current_exe_name = current_exe_path()?
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "解析当前可执行文件名失败".to_string())?
            .to_string();
        let _ = resolve_portable_restart_exe(&staging_dir, &current_exe_name)?;
        pending.staging_dir = Some(staging_dir.display().to_string());
    } else {
        pending.installer_path = Some(payload_path.display().to_string());
        #[cfg(target_os = "macos")]
        if is_dmg_asset(&payload_asset.name) {
            let staging_dir = release_dir.join("staging");
            if staging_dir.is_dir() {
                fs::remove_dir_all(&staging_dir)
                    .map_err(|err| format!("清理暂存目录失败：{err}"))?;
            }
            fs::create_dir_all(&staging_dir).map_err(|err| format!("创建暂存目录失败：{err}"))?;
            stage_macos_dmg_payload(&payload_path, &staging_dir, &prepare_log_path)?;
            pending.staging_dir = Some(staging_dir.display().to_string());
        }
    }

    write_pending_update(app, &pending)?;
    append_prepare_log(&prepare_log_path, "待更新状态已写入 pending-update.json");

    Ok(UpdatePrepareResponse {
        prepared: true,
        mode: context.check.mode,
        is_portable: context.check.is_portable,
        release_tag: context.check.release_tag,
        latest_version: context.check.latest_version,
        asset_name: pending.asset_name,
        asset_path: pending.asset_path,
        downloaded: true,
    })
}

#[cfg(test)]
mod tests {
    use super::{portable_asset_names_for_platform, sanitize_tag, select_macos_dmg_asset_for_arch};
    use crate::commands::updater::model::GitHubAsset;

    /// 函数 `portable_asset_names_include_current_workflow_artifact`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn portable_asset_names_include_current_workflow_artifact() {
        let names = portable_asset_names_for_platform("0.1.8");
        if cfg!(target_os = "windows") {
            assert!(names.iter().any(|name| name == "CodexManager-portable.exe"));
        } else if cfg!(target_os = "linux") {
            assert!(names
                .iter()
                .any(|name| name == "CodexManager-linux-portable.zip"));
        } else if cfg!(target_os = "macos") {
            assert!(names
                .iter()
                .any(|name| name == "CodexManager-macos-portable.zip"));
        }
    }

    /// 函数 `sanitize_tag_replaces_unsafe_characters`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn sanitize_tag_replaces_unsafe_characters() {
        assert_eq!(sanitize_tag("v0.1.8/beta"), "v0.1.8_beta");
    }

    /// 函数 `macos_dmg_selection_prefers_matching_arch_suffix`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn macos_dmg_selection_prefers_matching_arch_suffix() {
        let assets = vec![
            GitHubAsset {
                name: "CodexManager_0.1.8_aarch64.dmg".to_string(),
                browser_download_url: "https://example.com/arm.dmg".to_string(),
            },
            GitHubAsset {
                name: "CodexManager_0.1.8_x64.dmg".to_string(),
                browser_download_url: "https://example.com/x64.dmg".to_string(),
            },
        ];

        let selected =
            select_macos_dmg_asset_for_arch(&assets, &["x64", "x86_64"]).expect("x64 dmg");
        assert_eq!(selected.name, "CodexManager_0.1.8_x64.dmg");
    }

    /// 函数 `macos_dmg_selection_falls_back_to_generic_dmg`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn macos_dmg_selection_falls_back_to_generic_dmg() {
        let assets = vec![
            GitHubAsset {
                name: "CodexManager_0.1.8_aarch64.dmg".to_string(),
                browser_download_url: "https://example.com/arm.dmg".to_string(),
            },
            GitHubAsset {
                name: "CodexManager_0.1.8.dmg".to_string(),
                browser_download_url: "https://example.com/generic.dmg".to_string(),
            },
        ];

        let selected =
            select_macos_dmg_asset_for_arch(&assets, &["x64", "x86_64"]).expect("generic dmg");
        assert_eq!(selected.name, "CodexManager_0.1.8.dmg");
    }
}
