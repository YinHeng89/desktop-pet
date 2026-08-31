//! 本地通知 HTTP 服务。
//!
//! 监听 127.0.0.1:8756，接收 `POST /notify`（JSON），
//! 解析后通过 `app.emit("notify-push", payload)` 广播给前端宠物窗口。
//!
//! 这样任意外部应用/脚本（worktrack、curl、Python、Node...）都能
//! 用标准 HTTP 调用来给宠物发通知，语言无关。
//!
//! 用标准库 TcpListener 手写极简 HTTP（只解析 POST /notify 的 JSON body），
//! 避免引入额外依赖。
//!
//! # 安全边界
//!
//! 本服务暴露在本机端口上，**任何能访问该端口的程序都能让宠物说话**，
//! 因此必须防住两类典型攻击：
//!
//! 1. **浏览器发起的跨站请求 / DNS-rebinding**：任意网页都能对
//!    `http://127.0.0.1:8756/notify` 发 `POST`（用 `Content-Type: text/plain`
//!    即为 CORS 简单请求，无预检）。故必须校验 `Host` 头只接受回环地址。
//! 2. **资源耗尽**：`Content-Length` 可声明任意大值，慢速攻击可持续占用连接。
//!    故必须限制 body 大小、header 大小、单连接总耗时与并发连接数。
//!
//! 上述校验的纯逻辑（find_subslice / header_value / content_length /
//! is_allowed_host）已抽离到 `domain::notify::http_request`，本文件末尾的
//! `tests` 也随之迁往该模块；此处只负责 socket IO、限流与事件广播。

use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tauri::Emitter;

use crate::domain::notify::http_request::{content_length, find_subslice, header_value, is_allowed_host};

const PORT: u16 = 8756;
/// 通知文本字数硬上限（中文按 1 字符计），超出拒绝并返回错误提示。
const MAX_LEN: usize = 120;

/// 请求头大小上限。超过直接拒绝，避免无 \r\n\r\n 结尾的数据把内存吃满。
const MAX_HEADER_BYTES: usize = 8 * 1024;
/// 请求体大小上限。本服务只需要收一条几十字节的 JSON，8KB 绰绰有余。
const MAX_BODY_BYTES: usize = 8 * 1024;
/// 单次 read 的超时。
const READ_TIMEOUT: Duration = Duration::from_secs(3);
/// 单连接总耗时上限。
///
/// 注意：`set_read_timeout` 只约束**单次** read，攻击者每次只发 1 字节
/// 就能无限续期，因此必须另设一个总的截止时间（慢速攻击防护）。
const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
/// 并发连接上限。超出后新连接直接关闭，避免被打满线程池。
const MAX_CONNECTIONS: usize = 32;

/// 当前活跃连接数（配合 MAX_CONNECTIONS 使用）。
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

/// 连接计数守卫：Drop 时自动递减，避免线程 panic 导致计数只增不减。
struct ConnectionGuard;
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// 前端直接调用：通过 Rust 广播通知给 main 窗口的宠物气泡。
/// 走 Tauri IPC（invoke），绕过 HTTP/CORS，与 notify_server 广播同一事件名。
#[tauri::command]
pub fn push_notify(
    app: tauri::AppHandle,
    text: String,
    action: Option<String>,
    duration: Option<u64>,
) -> Result<(), String> {
    if text.is_empty() {
        return Err("通知文本不能为空".to_string());
    }
    // 字数硬限制：超过 MAX_LEN 个字符（中文按 1 字符计）拒绝，返回错误提示。
    if text.chars().count() > MAX_LEN {
        return Err(format!(
            "通知文本超限：最多 {} 字，当前 {} 字",
            MAX_LEN,
            text.chars().count()
        ));
    }
    let payload = serde_json::json!({
        "text": text,
        "action": action,
        "duration": duration,
    });
    let _ = app.emit("notify-push", payload);
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// 纯请求解析逻辑已迁入 domain::notify::http_request（find_subslice /
// header_value / content_length / is_allowed_host），本文件只负责
// socket IO、连接限流与事件广播。
// ─────────────────────────────────────────────────────────────

/// 写回一个无 body 的响应。
fn write_status(stream: &mut std::net::TcpStream, status: &str) -> io::Result<()> {
    stream.write_all(
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
    )
}

/// 写回一个 JSON 响应。
fn write_json(stream: &mut std::net::TcpStream, status: &str, body: &str) -> io::Result<()> {
    stream.write_all(
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .as_bytes(),
    )
}

/// 启动本地通知服务（阻塞，需在独立线程调用）。
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[notify-server] 绑定端口 {PORT} 失败: {e}");
                // 上报前端：端口被占用时用户此前完全无感知，
                // 只会表现为「通知发不出去」却不知道原因。
                let _ = app.emit(
                    "notify-server-error",
                    serde_json::json!({ "port": PORT, "error": e.to_string() }),
                );
                return;
            }
        };
        println!("[notify-server] 已监听 http://127.0.0.1:{PORT}/notify");

        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };

            // 并发上限：超出直接关闭新连接。
            // fetch_add 返回的是自增前的值，故用 >= 判断。
            if ACTIVE_CONNECTIONS.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
                ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
                continue;
            }

            let app = app.clone();
            std::thread::spawn(move || {
                let _guard = ConnectionGuard;
                let _ = handle(&mut stream, &app);
            });
        }
    });
}

/// 处理单个连接。
///
/// 读取顺序刻意安排为「边读边卡上限」，而不是先读满再校验：
/// 每次 read 后先检查总截止时间（防慢速攻击）；header 未收完但已超
/// MAX_HEADER_BYTES 时返回 431；header 收完后立即校验 Host（403）与
/// Content-Length（413）；之后才按声明的 body 长度继续读取，且总字节数
/// 不超过 header+body 上限。
fn handle(stream: &mut std::net::TcpStream, app: &tauri::AppHandle) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let deadline = Instant::now() + TOTAL_TIMEOUT;

    // ── 1) 读取请求头 ──
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let header_end = loop {
        if Instant::now() >= deadline {
            return write_status(stream, "408 Request Timeout");
        }
        match stream.read(&mut buf) {
            Ok(0) => return Ok(()), // 对端关闭
            Ok(n) => {
                // 硬上限：header 阶段不允许累积超过 MAX_HEADER_BYTES + MAX_BODY_BYTES。
                // （此时尚不知道 body 长度，先按两者之和兜底。）
                if data.len() + n > MAX_HEADER_BYTES + MAX_BODY_BYTES {
                    return write_status(stream, "413 Payload Too Large");
                }
                data.extend_from_slice(&buf[..n]);
                if let Some(i) = find_subslice(&data, b"\r\n\r\n") {
                    if i > MAX_HEADER_BYTES {
                        return write_status(stream, "431 Request Header Fields Too Large");
                    }
                    break i;
                }
            }
            // set_read_timeout 触发时返回 WouldBlock / TimedOut
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                return write_status(stream, "408 Request Timeout")
            }
            Err(e) => return Err(e),
        }
    };

    let header_text = String::from_utf8_lossy(&data[..header_end]).to_string();

    // ── 2) 安全校验：Host（防 DNS-rebinding / 浏览器跨站）──
    if !is_allowed_host(header_value(&header_text, "host")) {
        return write_status(stream, "403 Forbidden");
    }

    // ── 3) 安全校验：body 大小 ──
    let content_len = content_length(&header_text).unwrap_or(0);
    if content_len > MAX_BODY_BYTES {
        return write_status(stream, "413 Payload Too Large");
    }

    // ── 4) 按 Content-Length 补齐 body ──
    let body_start = header_end + 4;
    while data.len() < body_start + content_len {
        if Instant::now() >= deadline {
            return write_status(stream, "408 Request Timeout");
        }
        match stream.read(&mut buf) {
            Ok(0) => break, // 对端提前关闭
            Ok(n) => {
                if data.len() + n > MAX_HEADER_BYTES + MAX_BODY_BYTES {
                    return write_status(stream, "413 Payload Too Large");
                }
                data.extend_from_slice(&buf[..n]);
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                return write_status(stream, "408 Request Timeout")
            }
            Err(e) => return Err(e),
        }
    }

    let req = String::from_utf8_lossy(&data).to_string();

    // 解析请求行
    let request_line = req.lines().next().unwrap_or("");
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    // 只有 POST /notify 才处理，其余返回 404
    if method != "POST" || path != "/notify" {
        return write_status(stream, "404 Not Found");
    }

    // 提取 body（\r\n\r\n 之后）
    let body = req.split("\r\n\r\n").nth(1).unwrap_or("");

    // 解析 JSON
    let payload: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return write_status(stream, "400 Bad Request"),
    };

    // 提取字段
    let text = payload
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let action = payload
        .get("action")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let duration = payload.get("duration").and_then(|v| v.as_u64());

    if text.is_empty() {
        return write_status(stream, "400 Bad Request");
    }

    // 字数硬限制：超过 MAX_LEN 个字符（中文按 1 字符计）拒绝，返回 JSON 错误提示，
    // 调用方（curl/Python 等）可解析 error 字段拿到原因。
    if text.chars().count() > MAX_LEN {
        let body = serde_json::json!({
            "ok": false,
            "error": format!(
                "通知文本超限：最多 {} 字，当前 {} 字",
                MAX_LEN,
                text.chars().count()
            ),
        })
        .to_string();
        return write_json(stream, "400 Bad Request", &body);
    }

    // 广播给前端
    let emit_payload = serde_json::json!({
        "text": text,
        "action": action,
        "duration": duration,
    });
    let _ = app.emit("notify-push", emit_payload);

    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
}
