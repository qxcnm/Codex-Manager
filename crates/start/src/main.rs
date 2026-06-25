use std::fs;
#[cfg(target_os = "windows")]
use std::io;
use std::io::Write;
use std::net::TcpStream;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const ENV_CANDIDATES: [&str; 3] = ["codexmanager.env", "CodexManager.env", ".env"];
const DEFAULT_SERVICE_ADDR: &str = "localhost:48760";

#[cfg(target_os = "windows")]
mod windows_job {
    use super::*;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub(super) struct ChildJob {
        handle: HANDLE,
    }

    impl ChildJob {
        /// 函数 `new`
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
        pub(super) fn new() -> io::Result<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                unsafe {
                    CloseHandle(handle);
                }
                return Err(io::Error::last_os_error());
            }

            Ok(Self { handle })
        }

        /// 函数 `assign`
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
        pub(super) fn assign(&self, child: &Child) -> io::Result<()> {
            let process_handle = child.as_raw_handle() as HANDLE;
            let ok = unsafe { AssignProcessToJobObject(self.handle, process_handle) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for ChildJob {
        /// 函数 `drop`
        ///
        /// 作者: gaohongshun
        ///
        /// 时间: 2026-04-02
        ///
        /// # 参数
        /// - self: 参数 self
        ///
        /// # 返回
        /// 无
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    CloseHandle(self.handle);
                }
            }
        }
    }
}

/// 函数 `exe_dir`
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
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 函数 `strip_inline_comment`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn strip_inline_comment(value: &str) -> &str {
    // 仅把 ` #` 视为注释起点，保持与常见 dotenv 行为一致
    let Some(pos) = value.find(" #") else {
        return value;
    };
    value[..pos].trim_end()
}

/// 函数 `parse_dotenv_kv`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - line: 参数 line
///
/// # 返回
/// 返回函数执行结果
fn parse_dotenv_kv(line: &str) -> Option<(String, String)> {
    let mut line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
        return None;
    }
    if let Some(rest) = line.strip_prefix("export ") {
        line = rest.trim();
    }
    let (key, raw_value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut value = raw_value.trim();
    if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
        || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
    {
        value = &value[1..value.len() - 1];
    } else {
        value = strip_inline_comment(value);
    }
    Some((key.to_string(), value.to_string()))
}

/// 函数 `find_env_file_in_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - dir: 参数 dir
///
/// # 返回
/// 返回函数执行结果
fn find_env_file_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in ENV_CANDIDATES {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 函数 `load_env_from_exe_dir_best_effort`
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
fn load_env_from_exe_dir_best_effort() {
    let dir = exe_dir();
    let Some(path) = find_env_file_in_dir(&dir) else {
        return;
    };

    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };

    for line in text.lines() {
        let Some((key, value)) = parse_dotenv_kv(line) else {
            continue;
        };
        if std::env::var_os(&key).is_some() {
            continue;
        }
        std::env::set_var(key, value);
    }
}

/// 函数 `normalize_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_addr(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("http://") {
        value = rest;
    }
    if let Some(rest) = value.strip_prefix("https://") {
        value = rest;
    }
    value = value.split('/').next().unwrap_or(value);
    if value.is_empty() {
        return None;
    }
    if value.parse::<u16>().is_ok() {
        return Some(format!("localhost:{value}"));
    }
    Some(value.to_string())
}

/// 函数 `resolve_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - var: 参数 var
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
fn resolve_addr(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(|| default.to_string())
}

/// 函数 `resolve_web_addr`
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
fn resolve_web_addr() -> String {
    std::env::var("CODEXMANAGER_WEB_ADDR")
        .ok()
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(codexmanager_service::default_web_listener_addr)
}

/// 函数 `normalize_connect_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_connect_addr(raw: &str) -> String {
    let normalized = normalize_addr(raw).unwrap_or_else(|| raw.trim().to_string());
    let Some((host, port)) = normalized.rsplit_once(':') else {
        return normalized;
    };
    match host {
        "0.0.0.0" | "::" | "[::]" => format!("localhost:{port}"),
        _ => normalized,
    }
}

/// 函数 `browser_open_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn browser_open_addr(raw: &str) -> String {
    let normalized = normalize_addr(raw).unwrap_or_else(|| raw.trim().to_string());
    let Some((host, port)) = normalized.rsplit_once(':') else {
        return normalized;
    };
    match host {
        "0.0.0.0" | "::" | "[::]" => format!("127.0.0.1:{port}"),
        _ => normalized,
    }
}

/// 函数 `resolve_socket_addrs_best_effort`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - host_port: 参数 host_port
///
/// # 返回
/// 返回函数执行结果
fn resolve_socket_addrs_best_effort(host_port: &str) -> Vec<SocketAddr> {
    // 优先处理 localhost（避免 DNS 差异/大小写问题）
    let trimmed = host_port.trim();
    if trimmed.len() > "localhost:".len()
        && trimmed[..("localhost:".len())].eq_ignore_ascii_case("localhost:")
    {
        let port = &trimmed["localhost:".len()..];
        if let Ok(port) = port.parse::<u16>() {
            return vec![
                SocketAddr::from(([127, 0, 0, 1], port)),
                SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
            ];
        }
    }

    host_port
        .to_socket_addrs()
        .ok()
        .into_iter()
        .flatten()
        .collect()
}

/// 函数 `tcp_probe`
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
fn tcp_probe(addr: &str) -> bool {
    let addr = addr.trim();
    if addr.is_empty() {
        return false;
    }
    let addr = addr.strip_prefix("http://").unwrap_or(addr);
    let addr = addr.strip_prefix("https://").unwrap_or(addr);
    let addr = addr.split('/').next().unwrap_or(addr);

    for sock in resolve_socket_addrs_best_effort(addr) {
        if TcpStream::connect_timeout(&sock, Duration::from_millis(250)).is_ok() {
            return true;
        }
    }
    false
}

fn http_get_status_ok(addr: &str, path: &str, timeout: Duration) -> bool {
    let addr_trimmed = addr.trim();
    if addr_trimmed.is_empty() {
        return false;
    }
    let addr_trimmed = addr_trimmed.strip_prefix("http://").unwrap_or(addr_trimmed);
    let addr_trimmed = addr_trimmed
        .strip_prefix("https://")
        .unwrap_or(addr_trimmed);
    let addr_trimmed = addr_trimmed.split('/').next().unwrap_or(addr_trimmed);
    let Some(sock) = resolve_socket_addrs_best_effort(addr_trimmed)
        .into_iter()
        .next()
    else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&sock, timeout) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(timeout));
    let _ = stream.set_read_timeout(Some(timeout));
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr_trimmed}\r\nConnection: close\r\n\r\n");
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }

    let mut response = [0_u8; 16];
    let Ok(read) = std::io::Read::read(&mut stream, &mut response) else {
        return false;
    };
    read >= 12 && response.starts_with(b"HTTP/1.") && response[9] == b'2'
}

fn wait_for_service_ready(addr: &str, attempts: usize) -> bool {
    for _ in 0..attempts {
        if http_get_status_ok(addr, "/health", Duration::from_millis(750)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

fn simple_get_best_effort(addr: &str, path: &str) {
    let addr_trimmed = addr.trim();
    if addr_trimmed.is_empty() {
        return;
    }
    let addr_trimmed = addr_trimmed.strip_prefix("http://").unwrap_or(addr_trimmed);
    let addr_trimmed = addr_trimmed
        .strip_prefix("https://")
        .unwrap_or(addr_trimmed);
    let addr_trimmed = addr_trimmed.split('/').next().unwrap_or(addr_trimmed);
    let Some(sock) = resolve_socket_addrs_best_effort(addr_trimmed)
        .into_iter()
        .next()
    else {
        return;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&sock, Duration::from_millis(300)) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr_trimmed}\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(req.as_bytes());
}

/// 函数 `wait_for_port_closed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - attempts: 参数 attempts
///
/// # 返回
/// 返回函数执行结果
fn wait_for_port_closed(addr: &str, attempts: usize) -> bool {
    for _ in 0..attempts {
        if !tcp_probe(addr) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !tcp_probe(addr)
}

/// 函数 `stop_existing_service_best_effort`
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
fn stop_existing_service_best_effort(addr: &str) -> bool {
    codexmanager_service::request_shutdown(addr);
    wait_for_port_closed(addr, 30)
}

/// 函数 `stop_existing_web_best_effort`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - bind_addr: 参数 bind_addr
/// - open_addr: 参数 open_addr
///
/// # 返回
/// 返回函数执行结果
fn stop_existing_web_best_effort(bind_addr: &str, open_addr: &str) -> bool {
    simple_get_best_effort(open_addr, "/__quit");
    if !open_addr.eq_ignore_ascii_case(bind_addr) {
        simple_get_best_effort(bind_addr, "/__quit");
    }
    wait_for_port_closed(open_addr, 30)
}

/// 函数 `bin_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - dir: 参数 dir
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn bin_path(dir: &Path, name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return dir.join(format!("{name}.exe"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        return dir.join(name);
    }
}

/// 函数 `spawn_child`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - bin: 参数 bin
/// - service_bind_addr: 参数 service_bind_addr
///
/// # 返回
/// 返回函数执行结果
fn spawn_child(bin: &Path, service_bind_addr: Option<&str>) -> std::io::Result<Child> {
    let mut cmd = Command::new(bin);
    if let Some(bind_addr) = service_bind_addr {
        cmd.env("CODEXMANAGER_SERVICE_ADDR", bind_addr);
    }
    cmd.spawn()
}

/// 函数 `main`
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
fn main() {
    // 让 start.exe 也支持同目录 env 文件，保持与 service/web 一致。
    load_env_from_exe_dir_best_effort();
    codexmanager_service::init_logging();
    // 进一步对齐 service/web 的便携化初始化，确保 DB/RPC token 落点一致。
    codexmanager_service::portable::bootstrap_current_process();
    let _ = codexmanager_service::initialize_storage_if_needed();
    codexmanager_service::sync_runtime_settings_from_storage();

    let dir = exe_dir();
    let configured_service_addr = resolve_addr("CODEXMANAGER_SERVICE_ADDR", DEFAULT_SERVICE_ADDR);
    let service_addr = normalize_connect_addr(&configured_service_addr);
    let service_bind_addr = codexmanager_service::listener_bind_addr(&service_addr);
    let web_addr = resolve_web_addr();
    let web_open_addr = browser_open_addr(&web_addr);

    let service_bin = bin_path(&dir, "codexmanager-service");
    let web_bin = bin_path(&dir, "codexmanager-web");
    #[cfg(target_os = "windows")]
    let child_job = match windows_job::ChildJob::new() {
        Ok(job) => Some(job),
        Err(err) => {
            eprintln!("创建 Windows 子进程回收句柄失败，关闭窗口时可能遗留后台进程：{err}");
            None
        }
    };

    println!("CodexManager 启动器");
    println!("- service: {service_addr} (bind {service_bind_addr})");
    if web_open_addr == web_addr {
        println!("- web:     http://{web_addr}/");
    } else {
        println!("- web:     bind http://{web_addr}/, open http://{web_open_addr}/");
    }
    println!("按 Ctrl+C 退出");

    if !web_bin.is_file() {
        eprintln!("缺少文件：{}", web_bin.display());
        std::process::exit(1);
    }

    if tcp_probe(&service_addr) {
        println!("检测到 service 已在运行，尝试重启以应用当前配置...");
        if !stop_existing_service_best_effort(&service_addr) {
            eprintln!("service 端口仍被占用，请先关闭旧实例：{service_addr}");
            std::process::exit(1);
        }
    }
    if tcp_probe(&service_addr) {
        eprintln!("service 端口仍被占用，请先关闭旧实例：{service_addr}");
        std::process::exit(1);
    }

    if !service_bin.is_file() {
        eprintln!("service 不可达且缺少文件：{}", service_bin.display());
        std::process::exit(1);
    }

    println!("正在启动 service...");
    let mut service_child = match spawn_child(&service_bin, Some(&service_bind_addr)) {
        Ok(child) => child,
        Err(err) => {
            eprintln!("启动 service 失败：{err}");
            std::process::exit(1);
        }
    };
    #[cfg(target_os = "windows")]
    if let Some(job) = child_job.as_ref() {
        if let Err(err) = job.assign(&service_child) {
            eprintln!("service 未能加入 Windows 回收句柄，关闭窗口时可能残留：{err}");
        }
    }

    println!("等待 service 就绪...");
    if !wait_for_service_ready(&service_addr, 120) {
        eprintln!("service 启动后仍未通过健康检查：{service_addr}");
        let _ = service_child.kill();
        std::process::exit(1);
    }

    if tcp_probe(&web_open_addr) || tcp_probe(&web_addr) {
        println!("检测到 web 已在运行，尝试重启以应用当前配置...");
        if !stop_existing_web_best_effort(&web_addr, &web_open_addr) {
            eprintln!("web 端口仍被占用，请先关闭旧实例：http://{web_open_addr}/");
            std::process::exit(1);
        }
    }

    println!("正在启动 web...");
    let mut web_cmd = Command::new(&web_bin);
    // 由 start.exe 统一管理 service，避免 web 进程重复拉起/竞态。
    web_cmd.env("CODEXMANAGER_WEB_NO_SPAWN_SERVICE", "1");
    // 让 web 使用与本进程解析到的一致地址，避免 env 文件/系统变量差异导致难以定位。
    web_cmd.env("CODEXMANAGER_SERVICE_ADDR", &service_addr);
    web_cmd.env("CODEXMANAGER_WEB_ADDR", &web_addr);

    let mut web_child = match web_cmd.spawn() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("启动 web 失败：{err}");
            std::process::exit(1);
        }
    };
    #[cfg(target_os = "windows")]
    if let Some(job) = child_job.as_ref() {
        if let Err(err) = job.assign(&web_child) {
            eprintln!("web 未能加入 Windows 回收句柄，关闭窗口时可能残留：{err}");
        }
    }

    let should_exit = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&should_exit);
        let _ = ctrlc::set_handler(move || {
            flag.store(true, Ordering::SeqCst);
        });
    }

    // 监督进程：Ctrl+C 或任一子进程退出则进入关闭流程。
    loop {
        if should_exit.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(Some(status)) = web_child.try_wait() {
            println!("web 已退出：{status}");
            break;
        }
        if let Ok(Some(status)) = service_child.try_wait() {
            println!("service 已退出：{status}");
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    println!("正在关闭...");

    // 先关 web，再关 service。
    simple_get_best_effort(&web_addr, "/__quit");
    simple_get_best_effort(&service_addr, "/__shutdown");

    // 最后兜底：短等后强杀
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let web_done = web_child.try_wait().ok().flatten().is_some();
        let service_done = service_child.try_wait().ok().flatten().is_some();
        if web_done && service_done {
            break;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = web_child.kill();
    let _ = service_child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 函数 `normalize_connect_addr_maps_all_interfaces_to_localhost`
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
    fn normalize_connect_addr_maps_all_interfaces_to_localhost() {
        assert_eq!(normalize_connect_addr("0.0.0.0:48760"), "localhost:48760");
        assert_eq!(normalize_connect_addr("[::]:48760"), "localhost:48760");
        assert_eq!(
            normalize_connect_addr("192.168.1.8:48760"),
            "192.168.1.8:48760"
        );
    }

    /// 函数 `browser_open_addr_maps_all_interfaces_to_loopback`
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
    fn browser_open_addr_maps_all_interfaces_to_loopback() {
        assert_eq!(browser_open_addr("0.0.0.0:48761"), "127.0.0.1:48761");
        assert_eq!(browser_open_addr("[::]:48761"), "127.0.0.1:48761");
        assert_eq!(browser_open_addr("192.168.1.8:48761"), "192.168.1.8:48761");
    }
}
