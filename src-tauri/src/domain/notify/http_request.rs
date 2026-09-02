//! 通知 HTTP 请求解析（纯函数，输入字节/字符串 → 输出解析结果）。
//!
//! 不碰 socket、不依赖 Tauri。安全边界校验（Host 白名单、body 大小、请求行）
//! 全部在此以纯函数实现，便于单测。

/// 在 haystack 中查找 needle 子切片，返回起始偏移（找不到返回 None）。
pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 从请求头文本中取出指定字段的值（字段名大小写不敏感），找不到返回 None。
///
/// 传入的文本可以包含请求行——请求行不含 `:`，自然不会被误匹配。
pub fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    let want = name.to_ascii_lowercase();
    headers.lines().find_map(|line| {
        let (key, val) = line.split_once(':')?;
        if key.trim().to_ascii_lowercase() == want {
            Some(val.trim())
        } else {
            None
        }
    })
}

/// 解析 Content-Length。缺失返回 None（等价于 0），值非法返回 None。
pub fn content_length(headers: &str) -> Option<usize> {
    header_value(headers, "content-length")?
        .parse::<usize>()
        .ok()
}

/// 校验 Host 头是否指向本机回环地址。
///
/// 这是防 DNS-rebinding 与浏览器跨站请求的关键一道关口：
/// 任意网页都能对 127.0.0.1 发简单请求，若不校验 Host，
/// 用户浏览恶意页面时宠物就会被随意操纵。
///
///   - HTTP/1.1 请求必带 Host，缺失即视为不合法
///   - 端口不参与比较（允许任意端口，便于测试与反向代理场景）
///   - IPv6 字面量的 `[::1]:8756` 形式需单独拆解
pub fn is_allowed_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    let host = host.trim();
    if host.is_empty() {
        return false;
    }
    let hostname = if let Some(rest) = host.strip_prefix('[') {
        // IPv6 字面量：[::1] 或 [::1]:8756
        match rest.split_once(']') {
            Some((addr, _port)) => addr,
            None => return false,
        }
    } else {
        host.split_once(':').map(|(h, _port)| h).unwrap_or(host)
    };
    matches!(
        hostname.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

/// 校验 `Origin` 头，阻断浏览器跨站请求。
///
/// `Host` 白名单（[`is_allowed_host`]）只能防 DNS-rebinding（恶意域名解析到
/// 127.0.0.1、浏览器仍带 `Host: evil.com`），**挡不住**「恶意网页直接 fetch 本机
/// 端口」——此时浏览器带的是 `Host: 127.0.0.1` 与 `Origin: https://evil.com`，
/// 前者恰好通过白名单，于是宠物被远程网页随意操纵。
///
/// 规则：
///   - 无 `Origin` ⟹ 非浏览器上下文（curl / Python / 服务端程序），放行；
///   - `Origin: null` 或空 ⟹ 沙箱 iframe、`data:` URI、CSP 受限源，拒绝；
///   - 其余必须是 `http(s)://` 的回环源（`127.0.0.1` / `localhost` / `::1`）。
///
/// 这样本地开发服（`http://localhost:5173`）仍可调通知接口，而 `https://evil.com`
/// 被拒。注意：任何能在本机监听端口的进程本就等效于「本地程序」，故允许回环源是预期行为。
pub fn is_allowed_origin(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true; // 无 Origin = 非浏览器（curl / Python / 服务端），放行
    };
    let origin = origin.trim();
    if origin.is_empty() || origin.eq_ignore_ascii_case("null") {
        return false; // 沙箱 / data: / CSP 产生的 null 源，拒绝
    }
    // 仅允许 http(s) 回环源
    let rest = match origin.strip_prefix("http://") {
        Some(r) => r,
        None => match origin.strip_prefix("https://") {
            Some(r) => r,
            None => return false,
        },
    };
    // 去掉路径部分，只取 host[:port]
    let host = rest.split('/').next().unwrap_or(rest);
    // 提取「地址」部分用于比较（去掉端口）：
    //   - IPv6 字面量 `[::1]:port`：先按 `]` 拆，地址在方括号内（不可按 `:` 拆，
    //     否则 `::1` 里的冒号会误拆成空串）；
    //   - 其余（IPv4 / 域名）：按第一个 `:` 拆，端口在其后。
    let addr = if let Some(inner) = host.strip_prefix('[') {
        match inner.split_once(']') {
            Some((a, _port_part)) => a,
            None => host,
        }
    } else {
        host.split_once(':').map(|(h, _port)| h).unwrap_or(host)
    };
    matches!(
        addr.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_subslice_basic() {
        assert_eq!(find_subslice(b"abc\r\n\r\ndef", b"\r\n\r\n"), Some(3));
        assert_eq!(find_subslice(b"nope", b"\r\n\r\n"), None);
        assert_eq!(find_subslice(b"ab", b"abcd"), None);
    }

    #[test]
    fn find_subslice_empty_needle_returns_none() {
        assert_eq!(find_subslice(b"abc", b""), None);
    }

    #[test]
    fn header_value_is_case_insensitive() {
        let h = "POST /notify HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 12";
        assert_eq!(header_value(h, "host"), Some("127.0.0.1"));
        assert_eq!(header_value(h, "HOST"), Some("127.0.0.1"));
        assert_eq!(header_value(h, "Content-LENGTH"), Some("12"));
    }

    #[test]
    fn header_value_missing_returns_none() {
        let h = "POST /notify HTTP/1.1\r\nHost: 127.0.0.1";
        assert_eq!(header_value(h, "x-custom"), None);
    }

    #[test]
    fn header_value_keeps_colon_in_value() {
        let h = "POST /notify HTTP/1.1\r\nHost: [::1]:8756";
        assert_eq!(header_value(h, "host"), Some("[::1]:8756"));
    }

    #[test]
    fn header_value_request_line_is_not_matched() {
        let h = "POST /notify HTTP/1.1\r\nHost: localhost";
        assert_eq!(header_value(h, "post /notify http/1.1"), None);
    }

    #[test]
    fn content_length_parsed() {
        let h = "POST /notify HTTP/1.1\r\nContent-Length: 42";
        assert_eq!(content_length(h), Some(42));
    }

    #[test]
    fn content_length_missing_or_invalid_is_none() {
        let h = "POST /notify HTTP/1.1\r\nHost: localhost";
        assert_eq!(content_length(h), None);
        assert_eq!(content_length("Content-Length: abc"), None);
        assert_eq!(content_length("Content-Length: -1"), None);
    }

    #[test]
    fn allows_loopback_hosts() {
        for h in [
            "127.0.0.1",
            "127.0.0.1:8756",
            "localhost",
            "localhost:8756",
            "[::1]",
            "[::1]:8756",
        ] {
            assert!(is_allowed_host(Some(h)), "应允许: {h}");
        }
    }

    #[test]
    fn rejects_non_loopback_hosts() {
        for h in [
            "evil.com",
            "127.0.0.1.evil.com",
            "192.168.1.5",
            "0.0.0.0",
            "[::2]",
            "",
            "   ",
        ] {
            assert!(!is_allowed_host(Some(h)), "应拒绝: {h:?}");
        }
    }

    #[test]
    fn rejects_missing_host() {
        assert!(!is_allowed_host(None));
    }

    #[test]
    fn host_comparison_ignores_case() {
        assert!(is_allowed_host(Some("LOCALHOST")));
        assert!(is_allowed_host(Some("LocalHost:8756")));
    }

    #[test]
    fn rejects_malformed_ipv6_literal() {
        assert!(!is_allowed_host(Some("[::1")));
    }

    #[test]
    fn origin_allows_non_browser_clients() {
        // 无 Origin（curl / Python / 服务端程序）放行
        assert!(is_allowed_origin(None));
    }

    #[test]
    fn origin_allows_loopback_browser_pages() {
        for o in [
            "http://127.0.0.1:8756",
            "http://localhost",
            "https://localhost:5173",
            "http://[::1]:5173",
        ] {
            assert!(is_allowed_origin(Some(o)), "应允许: {o}");
        }
    }

    #[test]
    fn origin_rejects_remote_and_null() {
        for o in [
            "https://evil.com",
            "http://192.168.1.5",
            "null",
            "",
            "file:///tmp/x",
            "ftp://127.0.0.1",
        ] {
            assert!(!is_allowed_origin(Some(o)), "应拒绝: {o}");
        }
    }
}
