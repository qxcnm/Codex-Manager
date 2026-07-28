use std::sync::Once;

static LOGGER_INIT: Once = Once::new();
static PANIC_HOOK_INIT: Once = Once::new();

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn init_logging() {
    LOGGER_INIT.call_once(|| {
        let filter = non_empty_env("RUST_LOG")
            .or_else(|| non_empty_env("CODEXMANAGER_LOG"))
            .unwrap_or_else(|| "info".to_string());
        let mut builder = env_logger::Builder::new();
        builder.parse_filters(&filter);
        builder.format_timestamp_secs();

        if let Some(style) =
            non_empty_env("RUST_LOG_STYLE").or_else(|| non_empty_env("CODEXMANAGER_LOG_STYLE"))
        {
            builder.parse_write_style(&style);
        }

        // The desktop host installs tauri-plugin-log before starting the embedded
        // service. In that mode SetLoggerError means a usable logger already exists.
        let _ = builder.try_init();
    });
    install_panic_log_hook();
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let message = if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    };
    let message = message.replace(['\r', '\n'], " ");
    let lower = message.to_ascii_lowercase();
    if [
        "access_token",
        "refresh_token",
        "id_token",
        "authorization",
        "cookie",
        "bearer ",
        "sk-",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return "<redacted sensitive panic message>".to_string();
    }
    message.chars().take(2_000).collect()
}

pub fn install_panic_log_hook() {
    PANIC_HOOK_INIT.call_once(|| {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("unnamed");
            let (file, line, column) = info
                .location()
                .map(|location| (location.file(), location.line(), location.column()))
                .unwrap_or(("<unknown>", 0, 0));
            log::error!(
                "event=panic thread={} file={} line={} column={} message={}",
                thread_name,
                file,
                line,
                column,
                panic_message(info.payload())
            );
            previous_hook(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::panic_message;

    #[test]
    fn panic_message_redacts_sensitive_values() {
        let payload: Box<dyn std::any::Any + Send> =
            Box::new("request failed with Authorization: Bearer secret-token");

        assert_eq!(
            panic_message(payload.as_ref()),
            "<redacted sensitive panic message>"
        );
    }

    #[test]
    fn panic_message_is_single_line_and_bounded() {
        let payload: Box<dyn std::any::Any + Send> =
            Box::new(format!("first\r\n{}", "x".repeat(2_100)));
        let message = panic_message(payload.as_ref());

        assert!(!message.contains(['\r', '\n']));
        assert_eq!(message.chars().count(), 2_000);
    }
}
