use rfd::FileDialog;

use crate::app_storage::{
    read_account_import_contents_from_directory, read_account_import_contents_from_files,
};
use crate::commands::shared::rpc_call_in_background;
use crate::rpc_client::rpc_call;

const MAX_IMPORT_RPC_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMPORT_ERROR_ITEMS: usize = 50;

fn estimate_import_request_bytes(contents: &[String]) -> Result<usize, String> {
    serde_json::to_vec(&serde_json::json!({ "contents": contents }))
        .map(|bytes| bytes.len())
        .map_err(|err| format!("serialize import batch failed: {err}"))
}

fn split_import_contents(contents: Vec<String>) -> Result<Vec<Vec<String>>, String> {
    let mut chunks = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for content in contents {
        let mut next = current.clone();
        next.push(content.clone());
        if !current.is_empty() && estimate_import_request_bytes(&next)? > MAX_IMPORT_RPC_BODY_BYTES
        {
            chunks.push(current);
            current = vec![content];
            if estimate_import_request_bytes(&current)? > MAX_IMPORT_RPC_BODY_BYTES {
                return Err("单条导入内容过大，请拆分后重试".to_string());
            }
            continue;
        }

        current = next;
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    Ok(chunks)
}

fn number_field(payload: &serde_json::Map<String, serde_json::Value>, key: &str) -> usize {
    payload
        .get(key)
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        .min(usize::MAX as u64) as usize
}

fn import_result_object(
    response: serde_json::Value,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    if let Some(error) = response.get("error") {
        return Err(format!("account/import failed: {error}"));
    }
    response
        .get("result")
        .and_then(|value| value.as_object())
        .cloned()
        .ok_or_else(|| "account/import returned invalid response".to_string())
}

fn merge_import_result(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: serde_json::Map<String, serde_json::Value>,
    index_offset: usize,
) {
    for key in ["total", "created", "updated", "failed"] {
        let merged = number_field(target, key).saturating_add(number_field(&source, key));
        target.insert(key.to_string(), serde_json::json!(merged));
    }

    let target_imported_ids = target
        .entry("imported_account_ids".to_string())
        .or_insert_with(|| serde_json::json!([]));
    if let Some(target_imported_ids) = target_imported_ids.as_array_mut() {
        let source_imported_ids = source
            .get("imported_account_ids")
            .or_else(|| source.get("importedAccountIds"))
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        for account_id in source_imported_ids {
            let Some(account_id) = account_id
                .as_str()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            if !target_imported_ids
                .iter()
                .any(|value| value.as_str() == Some(account_id))
            {
                target_imported_ids.push(serde_json::json!(account_id));
            }
        }
    }

    let Some(source_errors) = source.get("errors").and_then(|value| value.as_array()) else {
        return;
    };
    let target_errors = target
        .entry("errors".to_string())
        .or_insert_with(|| serde_json::json!([]));
    let Some(target_errors) = target_errors.as_array_mut() else {
        return;
    };
    for error in source_errors {
        if target_errors.len() >= MAX_IMPORT_ERROR_ITEMS {
            break;
        }
        let Some(error) = error.as_object() else {
            continue;
        };
        let index = error
            .get("index")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            .saturating_add(index_offset as u64);
        let message = error
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        target_errors.push(serde_json::json!({
            "index": index,
            "message": message
        }));
    }
}

fn import_account_contents(
    addr: Option<String>,
    contents: Vec<String>,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let batches = split_import_contents(contents)?;
    let mut merged = serde_json::Map::new();
    merged.insert("total".to_string(), serde_json::json!(0));
    merged.insert("created".to_string(), serde_json::json!(0));
    merged.insert("updated".to_string(), serde_json::json!(0));
    merged.insert("failed".to_string(), serde_json::json!(0));
    merged.insert("errors".to_string(), serde_json::json!([]));
    merged.insert("imported_account_ids".to_string(), serde_json::json!([]));

    let mut processed_items = 0usize;
    for batch in batches {
        let params = serde_json::json!({ "contents": batch });
        let response = rpc_call("account/import", addr.clone(), Some(params))?;
        let result = import_result_object(response)?;
        let batch_total = number_field(&result, "total");
        merge_import_result(&mut merged, result, processed_items);
        processed_items = processed_items.saturating_add(batch_total);
    }

    Ok(merged)
}

/// 函数 `service_account_import`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - contents: 参数 contents
/// - content: 参数 content
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_import(
    addr: Option<String>,
    contents: Option<Vec<String>>,
    content: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut payload_contents = contents.unwrap_or_default();
    if let Some(single) = content {
        if !single.trim().is_empty() {
            payload_contents.push(single);
        }
    }
    let params = serde_json::json!({ "contents": payload_contents });
    rpc_call_in_background("account/import", addr, Some(params)).await
}

/// 函数 `service_account_import_by_directory`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - _addr: 参数 _addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_import_by_directory(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let selected_dir = FileDialog::new()
            .set_title("选择账号导入目录")
            .pick_folder();
        let Some(dir_path) = selected_dir else {
            return Ok(serde_json::json!({
              "result": {
                "ok": true,
                "canceled": true
              }
            }));
        };

        let (json_files, contents) = read_account_import_contents_from_directory(&dir_path)?;
        let mut result = import_account_contents(addr, contents)?;
        result.insert(
            "directoryPath".to_string(),
            serde_json::json!(dir_path.to_string_lossy().to_string()),
        );
        result.insert("fileCount".to_string(), serde_json::json!(json_files.len()));
        Ok(serde_json::json!({
          "result": result
        }))
    })
    .await
    .map_err(|err| format!("service_account_import_by_directory task failed: {err}"))?
}

/// 函数 `service_account_import_by_file`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - _addr: 参数 _addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_import_by_file(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let selected_files = FileDialog::new()
            .set_title("选择账号导入文件")
            .add_filter("账号文件", &["json", "txt"])
            .pick_files();
        let Some(file_paths) = selected_files else {
            return Ok(serde_json::json!({
              "result": {
                "ok": true,
                "canceled": true
              }
            }));
        };

        let contents = read_account_import_contents_from_files(&file_paths)?;
        let file_paths = file_paths
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let mut result = import_account_contents(addr, contents)?;
        result.insert("fileCount".to_string(), serde_json::json!(file_paths.len()));
        result.insert("filePaths".to_string(), serde_json::json!(file_paths));
        Ok(serde_json::json!({
          "result": result
        }))
    })
    .await
    .map_err(|err| format!("service_account_import_by_file task failed: {err}"))?
}

/// 函数 `service_account_export_by_account_files`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_export_by_account_files(
    addr: Option<String>,
    selected_account_ids: Option<Vec<String>>,
    export_mode: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let selected_dir = FileDialog::new()
            .set_title("选择账号导出目录")
            .pick_folder();
        let Some(dir_path) = selected_dir else {
            return Ok(serde_json::json!({
              "result": {
                "ok": true,
                "canceled": true
              }
            }));
        };
        let params = serde_json::json!({
          "outputDir": dir_path.to_string_lossy().to_string(),
          "selectedAccountIds": selected_account_ids.unwrap_or_default(),
          "exportMode": export_mode.unwrap_or_else(|| "multiple".to_string())
        });
        rpc_call("account/export", addr, Some(params))
    })
    .await
    .map_err(|err| format!("service_account_export_by_account_files task failed: {err}"))?
}
