#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// 函数 `collect_json_files_recursively`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - root: 参数 root
/// - output: 参数 output
///
/// # 返回
/// 返回函数执行结果
fn collect_json_files_recursively(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(root).map_err(|err| format!("read dir failed ({}): {err}", root.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|err| format!("read dir entry failed ({}): {err}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files_recursively(&path, output)?;
            continue;
        }
        let is_json = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if is_json {
            output.push(path);
        }
    }
    Ok(())
}

/// 函数 `read_account_import_contents_from_directory`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn read_account_import_contents_from_directory(
    root: &Path,
) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let mut json_files = Vec::new();
    collect_json_files_recursively(root, &mut json_files)?;
    json_files.sort();

    let mut contents = Vec::with_capacity(json_files.len());
    for path in &json_files {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("read json file failed ({}): {err}", path.display()))?;
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            contents.push(trimmed.to_string());
        }
    }
    Ok((json_files, contents))
}

/// 函数 `read_account_import_contents_from_files`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn read_account_import_contents_from_files(
    files: &[PathBuf],
) -> Result<Vec<String>, String> {
    let mut contents = Vec::with_capacity(files.len());
    for path in files {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("read import file failed ({}): {err}", path.display()))?;
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            contents.push(trimmed.to_string());
        }
    }
    Ok(contents)
}
