//! webp 尺寸解析与 base64 编解码（纯函数）。
//!
//! 不碰文件系统、不依赖 Tauri。输入字节/字符串，输出结果，便于任意平台单测。

/// 极简 webp 尺寸解析（零依赖，只读文件头，不解码像素）。
///
/// webp 三种编码的尺寸都在头部：
/// VP8(lossy) 帧头第 6~9 字节为 14 位 width/height；VP8L(lossless) 头部含 14 位
/// width/height；VP8X(extended) 含 24 位 width-1/height-1。返回 (width, height)，
/// 无法识别时返回 None。
pub fn webp_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 30 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return None;
    }
    let chunk = &data[12..16];
    // chunk data 从 FourCC(12-15) 之后的「chunk size(16-19)」之后开始，即 data[20..]。
    // （之前误用 data[16..] 会把 4 字节 chunk size 也算进 payload，导致真实 webp
    // 尺寸被错误偏移 4 字节解析——这会拖累外部高清包的 row/count 越界修正。）
    let payload = &data[20..];
    match chunk {
        // VP8 （有损）关键帧：3 字节帧头 + 3 字节 start code(0x9d,0x01,0x2a)
        // + 2 字节宽 + 2 字节高，均为 16 位小端
        b"VP8 " => {
            if payload.len() < 11 {
                return None;
            }
            let w = (payload[6] as u32) | ((payload[7] as u32) << 8);
            let h = (payload[8] as u32) | ((payload[9] as u32) << 8);
            Some((w, h))
        }
        // VP8L （无损）：首字节 0x2f + 14 位宽-1 + 14 位高-1
        b"VP8L" => {
            if payload.len() < 5 {
                return None;
            }
            let b0 = payload[1] as u32;
            let b1 = payload[2] as u32;
            let b2 = payload[3] as u32;
            let b3 = payload[4] as u32;
            let w = (b0 | ((b1 & 0x3f) << 8)) + 1;
            let h = ((b1 >> 6) | (b2 << 2) | ((b3 & 0x0f) << 10)) + 1;
            Some((w, h))
        }
        // VP8X （扩展）：24 位宽-1 + 24 位高-1（位于 payload[4..10]）
        b"VP8X" => {
            if payload.len() < 10 {
                return None;
            }
            let w = (payload[4] as u32) | ((payload[5] as u32) << 8) | ((payload[6] as u32) << 16);
            let h = (payload[7] as u32) | ((payload[8] as u32) << 8) | ((payload[9] as u32) << 16);
            Some((w + 1, h + 1))
        }
        _ => None,
    }
}

/// 极简 base64 编码（标准库实现，避免额外依赖）。
pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // 每 3 字节编码为 4 字符；不足 3 字节的尾部按 1 组计（等价旧的 (len + 2) / 3）
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 极简 base64 解码（返回字节，忽略空白与非法字符）。
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> i32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as i32,
            b'a'..=b'z' => (c - b'a' + 26) as i32,
            b'0'..=b'9' => (c - b'0' + 52) as i32,
            b'+' => 62,
            b'/' => 63,
            _ => -1,
        }
    }
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &b in input.as_bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' || b == b' ' {
            continue;
        }
        let v = val(b);
        if v < 0 {
            return Err("base64 含非法字符".into());
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webp_vp8_lossy_dims() {
        // 构造最小 VP8 有损头：RIFF...WEBP VP8 + 10 字节 chunk size，
        // 之后是 VP8 比特流：3 字节帧头 + 3 字节 start code + 2 字节宽 + 2 字节高。
        // 注意：比特流从 data[20] 起始（即 payload），宽在 payload[6..8]、高在 payload[8..10]。
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&20u32.to_le_bytes());
        v.extend_from_slice(b"WEBP");
        v.extend_from_slice(b"VP8 ");
        v.extend_from_slice(&10u32.to_le_bytes()); // chunk size = data[16..20]
        v.extend_from_slice(&[0u8; 6]); // VP8 帧头(3) + start code(3) = data[20..26]
        v.extend_from_slice(&[0x40, 0x01]); // w = 0x140 = 320  (data[26..28])
        v.extend_from_slice(&[0x80, 0x01]); // h = 0x180 = 384  (data[28..30])
        v.extend_from_slice(&[0u8; 2]); // 补足到 32 字节（payload 需 ≥11）
        assert_eq!(webp_dimensions(&v), Some((320, 384)));
    }

    #[test]
    fn webp_vp8l_dims() {
        // VP8L：w-1=99, h-1=199 → (100, 200)。
        // 位打包：b0=w-1低8位；b1=((h-1&0x3)<<6)|(w-1>>8)；b2=(h-1>>2)&0xFF；b3=(h-1>>10)&0x0F
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&14u32.to_le_bytes());
        v.extend_from_slice(b"WEBP");
        v.extend_from_slice(b"VP8L");
        v.extend_from_slice(&5u32.to_le_bytes());
        v.push(0x2f); // 签名
        v.push(0x63); // b0 = 99
        v.push(0xc0); // b1 = ((199 & 3) << 6) | (99 >> 8) = 0xc0
        v.push(0x31); // b2 = (199 >> 2) & 0xFF = 49
        v.push(0x00); // b3 = (199 >> 10) & 0x0F = 0
        v.extend_from_slice(&[0u8; 5]); // 补足到 30 字节（满足 data.len() < 30 检查）
        assert_eq!(webp_dimensions(&v), Some((100, 200)));
    }

    #[test]
    fn webp_invalid_returns_none() {
        assert_eq!(webp_dimensions(b"not a webp at all"), None);
        assert_eq!(webp_dimensions(&[]), None);
    }

    #[test]
    fn base64_roundtrip() {
        let data: Vec<u8> = (0u8..=255).collect();
        let enc = base64_encode(&data);
        let dec = base64_decode(&enc).expect("decode ok");
        assert_eq!(dec, data);
        assert!(base64_decode("!!!not base64!!!").is_err());
    }
}
