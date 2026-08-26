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

use std::io::{Read, Write};
use std::net::TcpListener;
use tauri::Emitter;

const PORT: u16 = 8756;
/// 通知文本字数硬上限（中文按 1 字符计），超出拒绝并返回错误提示。
const MAX_LEN: usize = 120;

/// 前端直接调用：通过 Rust 广播通知给 main 窗口的宠物气泡。
/// 走 Tauri IPC（invoke），绕过 HTTP/CORS，与 notify_server 广播同一事件名。
#[tauri::command]
pub fn push_notify(app: tauri::AppHandle, text: String, action: Option<String>, duration: Option<u64>) -> Result<(), String> {
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

/// 在 haystack 中查找 needle 子切片，返回起始偏移（找不到返回 None）。
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// 启动本地通知服务（阻塞，需在独立线程调用）。
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[notify-server] 绑定端口 {} 失败: {e}", PORT);
                return;
            }
        };
        println!("[notify-server] 已监听 http://127.0.0.1:{}/notify", PORT);

        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let app = app.clone();
            std::thread::spawn(move || {
                let _ = handle(&mut stream, &app);
            });
        }
    });
}

fn handle(stream: &mut std::net::TcpStream, app: &tauri::AppHandle) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;

    // 循环读取直到读到完整的请求（header + body）。
    // 单次 read 可能只读到 header 或部分 body（TCP 分段），需按 Content-Length 补齐。
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break; // 对端关闭
        }
        data.extend_from_slice(&buf[..n]);
        // 已收到完整 header？
        if let Some(header_end) = find_subslice(&data, b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&data[..header_end]).to_string();
            let content_len = header
                .lines()
                .find_map(|l| {
                    let mut parts = l.splitn(2, ':');
                    let key = parts.next()?.trim().to_ascii_lowercase();
                    let val = parts.next()?.trim();
                    (key == "content-length").then(|| val.parse::<usize>().ok()).flatten()
                })
                .unwrap_or(0);
            // header 后的 body 字节数
            let body_start = header_end + 4;
            if data.len() >= body_start + content_len {
                break; // body 读完整了
            }
        }
    }

    let req = String::from_utf8_lossy(&data).to_string();

    // 解析请求行
    let request_line = req.lines().next().unwrap_or("");
    let mut method = "";
    let mut path = "";
    if let Some(first) = request_line.split(' ').next() {
        method = first;
    }
    if let Some(second) = request_line.split(' ').nth(1) {
        path = second;
    }

    // 只有 POST /notify 才处理，其余返回 404
    if method != "POST" || path != "/notify" {
        let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes());
        return Ok(());
    }

    // 提取 body（\r\n\r\n 之后）
    let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();

    // 解析 JSON
    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(resp.as_bytes());
            return Ok(());
        }
    };

    // 提取字段
    let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let action = payload.get("action").and_then(|v| v.as_str()).map(|s| s.to_string());
    let duration = payload.get("duration").and_then(|v| v.as_u64()).map(|d| d as u64);

    if text.is_empty() {
        let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes());
        return Ok(());
    }

    // 字数硬限制：超过 MAX_LEN 个字符（中文按 1 字符计）拒绝，返回 JSON 错误提示，
    // 调用方（curl/Python 等）可解析 error 字段拿到原因。
    if text.chars().count() > MAX_LEN {
        let msg = format!("通知文本超限：最多 {} 字，当前 {} 字", MAX_LEN, text.chars().count());
        let body = serde_json::json!({ "ok": false, "error": msg }).to_string();
        let resp = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.as_bytes().len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        return Ok(());
    }

    // 广播给前端
    let emit_payload = serde_json::json!({
        "text": text,
        "action": action,
        "duration": duration,
    });
    let _ = app.emit("notify-push", emit_payload);

    let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
    let _ = stream.write_all(resp.as_bytes());
    Ok(())
}
