/// 函数 `load_env_from_exe_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn load_env_from_exe_dir() -> Vec<(log::Level, String)> {
    let mut events = Vec::new();
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(err) => {
            events.push((
                log::Level::Warn,
                format!("event=env_executable_path_resolution_failed error={err}"),
            ));
            return events;
        }
    };
    let Some(exe_dir) = exe_path.parent() else {
        events.push((
            log::Level::Warn,
            "event=env_executable_directory_missing".to_string(),
        ));
        return events;
    };

    let candidates = ["codexmanager.env", "CodexManager.env", ".env"];
    let mut chosen = None;
    for name in candidates {
        let p = exe_dir.join(name);
        if p.is_file() {
            chosen = Some(p);
            break;
        }
    }
    let Some(path) = chosen else {
        return events;
    };

    let bytes = match std::fs::read(&path) {
        Ok(v) => v,
        Err(err) => {
            events.push((
                log::Level::Warn,
                format!(
                    "event=env_file_read_failed path={} error={err}",
                    path.display()
                ),
            ));
            return events;
        }
    };
    let content = String::from_utf8_lossy(&bytes);
    let mut applied = 0usize;
    for (idx, raw_line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key_raw, value_raw)) = line.split_once('=') else {
            events.push((
                log::Level::Warn,
                format!(
                    "event=env_line_skipped path={} line={} reason=missing_separator",
                    path.display(),
                    line_no
                ),
            ));
            continue;
        };
        let key = key_raw.trim();
        if key.is_empty() {
            continue;
        }
        let mut value = value_raw.trim().to_string();
        if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            value = value[1..value.len() - 1].to_string();
        }

        if std::env::var_os(key).is_some() {
            continue;
        }
        std::env::set_var(key, value);
        applied += 1;
    }

    if applied > 0 {
        events.push((
            log::Level::Info,
            format!(
                "event=env_file_loaded path={} applied={applied}",
                path.display()
            ),
        ));
    }
    events
}
