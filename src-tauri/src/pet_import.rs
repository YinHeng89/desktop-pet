// 外部宠物导入：把用户选择的 .zip 压缩包解压到 app_data_dir/pets/<id>/，
// 读取其中的 pet.json（Codex 格式），套用标准精灵图布局（192x208、8 列），
// 返回宠物定义给前端注册进 petStore。
//
// zip 结构约定（与内置宠物包一致）：
//   pet.json           # 必填：{ id, displayName, description, spritesheetPath }
//   spritesheet.webp   # 必填：精灵图
//
// 帧布局默认套用 Codex Pet V2 标准（与 manifest.json 内置宠物一致）：
//   idle=row0/6帧；talk=row3/4帧；
//   actions: wave/row3、jump/row4、failed/row5、waiting/row6、look/row9。
// 若 pet.json 额外提供 idle / talk / actions 字段则覆盖默认值。
//
// spritesheet 以 base64 data URL 返回（避免引入 asset 协议配置），前端直接可用。

use serde::Serialize;
use std::io::Read;
use tauri::{Emitter, Manager};

// 返回给前端的宠物定义
#[derive(Serialize, Clone)]
pub struct FrameSeqJson {
    pub row: u32,
    pub count: u32,
    pub fps: u32,
}

/// 单帧几何信息：前端按此切分精灵图（CSS background-position / canvas drawImage）。
///
/// 之前「帧尺寸」是写死的全局常量（192×208、8 列），所有宠物共用。
/// 这让非标准外部包（如高清帧、或 256×256/6 列的包）切帧错位。
/// 现在每个宠物独立携带自己的 frame：width/height 由 pet.json 的 frame 声明给出，
/// cols 由声明或精灵图实际宽度推导，rows 由精灵图实际高度 / 帧高得出。
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct PetFrameJson {
    pub width: u32,
    pub height: u32,
    pub cols: u32,
    pub rows: u32,
}

#[derive(Serialize, Clone)]
pub struct PetDefJson {
    pub id: String,
    pub display_name: String,
    pub description: String,
    /// spritesheet 的 base64 data URL（data:image/webp;base64,...）
    pub spritesheet: String,
    pub idle: FrameSeqJson,
    pub talk: FrameSeqJson,
    pub actions: std::collections::BTreeMap<String, FrameSeqJson>,
    /// 该宠物自己的单帧几何（见 PetFrameJson 说明）。
    pub frame: PetFrameJson,
}

#[derive(serde::Deserialize)]
struct RawPetJson {
    id: String,
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    idle: Option<RawSeq>,
    #[serde(default)]
    talk: Option<RawSeq>,
    #[serde(default)]
    actions: Option<std::collections::BTreeMap<String, RawSeq>>,
    /// 可选的帧几何声明。缺失时由精灵图实际尺寸回退推导（见 compute_frame）。
    #[serde(default)]
    frame: Option<RawFrame>,
}

/// 外部 pet.json 里的帧几何声明（可选）。
#[derive(serde::Deserialize, Clone)]
struct RawFrame {
    width: u32,
    height: u32,
    cols: u32,
}
#[derive(serde::Deserialize, Clone)]
struct RawSeq {
    row: u32,
    count: u32,
    fps: u32,
}

fn seq(row: u32, count: u32, fps: u32) -> FrameSeqJson {
    FrameSeqJson { row, count, fps }
}

/// 极简 webp 尺寸解析（零依赖，只读文件头，不解码像素）。
///
/// webp 三种编码的尺寸都在头部：VP8(lossy) 帧头第 6~9 字节为 14 位
/// width/height；VP8L(lossless) 头部含 14 位 width/height；VP8X(extended)
/// 含 24 位 width-1/height-1。返回 (width, height)，无法识别时返回 None。
fn webp_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    // RIFF 头：'RIFF' + size(4) + 'WEBP'
    if data.len() < 30 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return None;
    }
    let chunk = &data[12..16];
    let payload = &data[16..];
    match chunk {
        b"VP8 " => {
            // 帧头：3 字节 tag + 3 字节 start code + 2 字节 width + 2 字节 height
            // 注意:VP8 (lossy) 的宽高是 16 位小端的「实际像素值」(非减一,也非 14 位)。
            // 之前误用 VP8L 的 14 位掩码 & 0x3fff 会丢弃高位,宽度 >=16384 时解析错误。
            // 此处直接取 16 位完整值。
            if payload.len() < 10 {
                return None;
            }
            let w = u16::from_le_bytes([payload[6], payload[7]]) as u32;
            let h = u16::from_le_bytes([payload[8], payload[9]]) as u32;
            Some((w, h))
        }
        b"VP8L" => {
            if payload.len() < 5 {
                return None;
            }
            // 字节 1~4：width-1 (14 bit) | height-1 (14 bit)
            let b0 = payload[1] as u32;
            let b1 = payload[2] as u32;
            let b2 = payload[3] as u32;
            let b3 = payload[4] as u32;
            let w = (b0 | ((b1 & 0x3f) << 8)) + 1;
            let h = (((b1 >> 6) & 0x03) | (b2 << 2) | ((b3 & 0x0f) << 10)) + 1;
            Some((w, h))
        }
        b"VP8X" => {
            if payload.len() < 10 {
                return None;
            }
            // 字节 4~6：24 位 width-1；字节 7~9：24 位 height-1
            let w = (payload[4] as u32) | ((payload[5] as u32) << 8) | ((payload[6] as u32) << 16);
            let h = (payload[7] as u32) | ((payload[8] as u32) << 8) | ((payload[9] as u32) << 16);
            Some((w + 1, h + 1))
        }
        _ => None,
    }
}

/// Rust 侧用于 row/count 越界估算的「默认假设」帧尺寸。
/// 注意：这并非强制标准。项目支持任意帧尺寸的外部包(Codex 生成的高清大帧
/// 尺寸远大于此、列数也非 8)，前端按 manifest 声明的真实 frame 尺寸自适应取帧。
/// 这里仅用于保守估算行数、以及 clamp_seq 对每行列数做上限保护。
const FRAME_H: u32 = 208;
const FRAME_COLS: u32 = 8;

/// 根据精灵图实际尺寸，修正一个帧段的 row/count，避免越界：
///
///   - row 超出实际行数 → 返回 None（该动作不可用，应移除）
///   - count / fps 为 0 → 用兜底值（0 帧或 0 fps 会导致动画卡死/不播放）
///   - count 超出该行剩余列数 → 截断到可用列数（用该宠物的真实列数，
///     而非写死的 FRAME_COLS，否则 17 列高清包的帧会被错误截断）
fn clamp_seq(s: FrameSeqJson, rows: u32, cols: u32) -> Option<FrameSeqJson> {
    if s.row >= rows {
        return None;
    }
    // 该行最多 cols 帧，count 不应超过
    let count = if s.count == 0 {
        cols
    } else {
        s.count.min(cols)
    };
    if count == 0 {
        return None;
    }
    // fps 下限保护：0 fps 会让 CSS 动画卡在第一帧，给一个合理默认
    let fps = if s.fps == 0 { 8 } else { s.fps };
    Some(FrameSeqJson {
        row: s.row,
        count,
        fps,
    })
}

fn default_actions() -> std::collections::BTreeMap<String, FrameSeqJson> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("wave".into(), seq(3, 4, 10));
    m.insert("jump".into(), seq(4, 5, 10));
    m.insert("failed".into(), seq(5, 8, 10));
    m.insert("waiting".into(), seq(6, 6, 8));
    m.insert("working".into(), seq(7, 6, 8));
    m.insert("look".into(), seq(9, 8, 8));
    // 拖动跑步（与内置宠物一致：row 1 向右跑、row 2 向左跑）
    m.insert("runningRight".into(), seq(1, 8, 10));
    m.insert("runningLeft".into(), seq(2, 8, 10));
    m
}

/// 极简 base64 编码（标准库实现，避免额外依赖）
fn base64_encode(data: &[u8]) -> String {
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

/// 极简 base64 解码（返回字节，忽略空白与非法字符）
fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
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

/// 计算该宠物的单帧几何（PetFrameJson）。
///
/// 声明优先：pet.json 显式声明 frame{width,height,cols} 时直接采用，
/// rows 由精灵图实际高度 / 帧高推导（声明宽度/高度/列数若为 0 则回退到默认值）。
/// 未声明时回退到默认 192×208 / 8 列，rows 由精灵图实际高度 / 208 推导。
///
/// 注意：sheet 实际尺寸只在「解析失败」时才回退（那才是真问题，
/// 见 build_pet_def 的告警），正常包都能拿到真实尺寸。
fn compute_frame(raw: &RawPetJson, sheet: Option<(u32, u32)>) -> PetFrameJson {
    let (sheet_w, sheet_h) = sheet.unwrap_or((192 * 8, 208 * 11));

    let declared = raw.frame.as_ref();
    // 声明值若为 0（非法）则回退到默认；用 max(1) 避免后续除法除零。
    let frame_h = declared.map(|f| f.height).unwrap_or(FRAME_H).max(1);
    let cols = declared.map(|f| f.cols).unwrap_or(FRAME_COLS).max(1);

    // 未声明宽时用「精灵图实际宽 / 列数」反推单帧宽，更贴合真实包。
    let frame_w = if let Some(f) = declared {
        if f.width == 0 {
            sheet_w / cols
        } else {
            f.width
        }
    } else {
        sheet_w / cols
    };

    // rows 由精灵图实际高度 / 帧高得出（frame_h 已 ≥1，无需再判零）
    let rows = (sheet_h / frame_h).max(1);

    PetFrameJson {
        width: frame_w,
        height: frame_h,
        cols,
        rows,
    }
}

/// 从 pet.json 内容 + 精灵图字节，组装宠物定义。
///
/// 关键：外部宠物精灵图行数/列数不统一（标准 11 行 vs 某些包 9 行、列数也非 8）。
/// 这里解析 webp 实际尺寸，算出真实行/列数，对默认模板的 row/count 做越界修正：
/// row 越界的动作会被移除（前端不会播放它，避免画布清空导致宠物消失），
/// count 越界则截断到该行可用列数（用该宠物的真实列数而非写死的 FRAME_COLS）。
/// 同时产出 per-pet frame 几何（见 compute_frame），让前端正确切帧。
fn build_pet_def(raw: &RawPetJson, spritesheet_bytes: &[u8]) -> PetDefJson {
    let sheet = webp_dimensions(spritesheet_bytes);
    if sheet.is_none() {
        eprintln!(
            "[pet_import] 警告(宠物 {}): 无法解析精灵图尺寸(非有效 webp?),按默认 8 列回退处理",
            raw.id
        );
    }
    let frame = compute_frame(raw, sheet);

    let idle_raw = raw
        .idle
        .as_ref()
        .map(|s| seq(s.row, s.count, s.fps))
        .unwrap_or_else(|| seq(0, 6, 8));
    let talk_raw = raw
        .talk
        .as_ref()
        .map(|s| seq(s.row, s.count, s.fps))
        .unwrap_or_else(|| seq(3, 4, 10));
    let actions_raw = raw
        .actions
        .as_ref()
        .map(|m| {
            m.iter()
                .map(|(k, s)| (k.clone(), seq(s.row, s.count, s.fps)))
                .collect()
        })
        .unwrap_or_else(default_actions);

    // 越界修正：idle/talk 必须有，越界则回退到 row 0（count 用真实列数兜底）
    let idle =
        clamp_seq(idle_raw, frame.rows, frame.cols).unwrap_or_else(|| seq(0, frame.cols.min(6), 8));
    let talk = clamp_seq(talk_raw, frame.rows, frame.cols).unwrap_or_else(|| idle.clone());
    // actions 逐个修正，越界的直接移除
    let actions: std::collections::BTreeMap<String, FrameSeqJson> = actions_raw
        .into_iter()
        .filter_map(|(k, s)| clamp_seq(s, frame.rows, frame.cols).map(|cs| (k, cs)))
        .collect();

    PetDefJson {
        id: raw.id.trim().to_string(),
        display_name: raw
            .display_name
            .clone()
            .unwrap_or_else(|| raw.id.trim().to_string()),
        description: raw.description.clone().unwrap_or_default(),
        spritesheet: format!(
            "data:image/webp;base64,{}",
            base64_encode(spritesheet_bytes)
        ),
        idle,
        talk,
        actions,
        frame,
    }
}

/// 外部宠物导入命令：解压 zip 到 app_data_dir/pets/<id>/，返回宠物定义。
#[tauri::command]
pub async fn import_pet(app: tauri::AppHandle, base64: String) -> Result<PetDefJson, String> {
    let bytes = base64_decode(&base64).map_err(|e| format!("zip 内容解码失败: {e}"))?;
    let app_for_emit = app.clone();

    let pets_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("pets");

    let root = pets_root.clone();
    let def = tauri::async_runtime::spawn_blocking(move || -> Result<PetDefJson, String> {
        let cursor = std::io::Cursor::new(bytes);
        let mut zip = zip::ZipArchive::new(cursor).map_err(|e| format!("zip 打开失败: {e}"))?;

        // 找到 pet.json 并解析
        let mut raw_json: Option<RawPetJson> = None;
        for i in 0..zip.len() {
            let is_pet_json = {
                let entry = zip.by_index(i).map_err(|e| format!("zip 读取失败: {e}"))?;
                entry.name().ends_with("pet.json")
            };
            if is_pet_json {
                let mut content = String::new();
                zip.by_index(i)
                    .map_err(|e| format!("zip 读取失败: {e}"))?
                    .read_to_string(&mut content)
                    .map_err(|e| format!("pet.json 读取失败: {e}"))?;
                raw_json = Some(
                    serde_json::from_str(&content)
                        .map_err(|e| format!("pet.json 解析失败: {e}"))?,
                );
                break;
            }
        }
        let raw = raw_json.ok_or("压缩包内未找到 pet.json")?;
        let id = raw.id.trim().to_string();
        if id.is_empty() {
            return Err("pet.json 缺少 id".into());
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("宠物 id 含非法字符".into());
        }

        // 解压全部条目，记录 webp 字节
        let target_dir = root.join(&id);
        if target_dir.exists() {
            std::fs::remove_dir_all(&target_dir).map_err(|e| format!("清理旧宠物目录失败: {e}"))?;
        }
        std::fs::create_dir_all(&target_dir).map_err(|e| format!("创建宠物目录失败: {e}"))?;

        let mut webp_bytes: Option<Vec<u8>> = None;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| format!("zip 读取失败: {e}"))?;
            let Some(rel) = entry.enclosed_name() else {
                continue;
            };
            let lower = rel.to_string_lossy().to_lowercase();
            let out = target_dir.join(&rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out).map_err(|e| format!("解压建目录失败: {e}"))?;
            } else {
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("解压建目录失败: {e}"))?;
                }
                let mut buf = Vec::with_capacity(entry.size() as usize);
                entry
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("解压读取失败: {e}"))?;
                std::fs::write(&out, &buf).map_err(|e| format!("解压写文件失败: {e}"))?;
                if lower.ends_with(".webp") {
                    webp_bytes = Some(buf);
                }
            }
        }

        let webp_bytes = webp_bytes.ok_or("压缩包内未找到 spritesheet.webp")?;
        Ok(build_pet_def(&raw, &webp_bytes))
    })
    .await
    .map_err(|e| format!("解压任务异常: {e}"))?;

    // 导入成功：通知主进程重建托盘菜单（切换宠物子菜单反映新宠物）
    let _ = app_for_emit.emit("pet-pets-changed", ());

    // 返回构建好的宠物定义（def 已是 Result<PetDefJson, String>，与本函数签名一致）
    def
}

/// 同步扫描 app_data_dir/pets/，返回已导入宠物的 (id, displayName) 列表。
/// 供托盘菜单动态构建「切换宠物」子菜单用（不读精灵图字节，只读 pet.json 元信息）。
pub fn list_imported_pet_meta(app: &tauri::AppHandle) -> Vec<(String, String)> {
    let Ok(pets_root) = app.path().app_data_dir() else {
        return Vec::new();
    };
    let pets_root = pets_root.join("pets");
    let Ok(entries) = std::fs::read_dir(&pets_root) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let pet_json_path = entry.path().join("pet.json");
        let Ok(content) = std::fs::read_to_string(&pet_json_path) else {
            continue;
        };
        let Ok(raw) = serde_json::from_str::<RawPetJson>(&content) else {
            continue;
        };
        let id = raw.id.trim().to_string();
        if id.is_empty() {
            continue;
        }
        let display_name = raw.display_name.clone().unwrap_or_else(|| id.clone());
        result.push((id, display_name));
    }
    result
}

/// 删除已导入的外部宠物：移除 app_data_dir/pets/<id>/ 目录。
#[tauri::command]
pub async fn delete_imported_pet(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let pets_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("pets");

    // id 白名单校验（防目录穿越）
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("宠物 id 含非法字符".into());
    }
    let app_for_emit = app.clone();

    let _ = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let target = pets_root.join(&id);
        if target.exists() {
            std::fs::remove_dir_all(&target).map_err(|e| format!("删除宠物目录失败: {e}"))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("删除任务异常: {e}"))?;

    // 删除成功：通知主进程重建托盘菜单
    let _ = app_for_emit.emit("pet-pets-changed", ());

    Ok(())
}

/// 编辑外部宠物元信息：仅更新 displayName / description，不动精灵图与帧布局，
/// 写回 pet.json 后通知主进程重建托盘菜单。
#[tauri::command]
pub async fn update_imported_pet(
    app: tauri::AppHandle,
    id: String,
    display_name: Option<String>,
    description: Option<String>,
) -> Result<(), String> {
    // id 白名单校验（防目录穿越）
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("宠物 id 含非法字符".into());
    }

    let pets_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("pets");
    let app_for_emit = app.clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let json_path = pets_root.join(&id).join("pet.json");
        if !json_path.exists() {
            return Err(format!("宠物不存在: {}", id));
        }
        let raw =
            std::fs::read_to_string(&json_path).map_err(|e| format!("读取宠物配置失败: {e}"))?;
        let mut pet: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("解析宠物配置失败: {}", e))?;

        if let Some(name) = display_name {
            let name = name.trim().to_string();
            // 空名字回退为原 displayName，避免清空
            let fallback = pet["displayName"]
                .as_str()
                .unwrap_or("外部宠物")
                .to_string();
            pet["displayName"] =
                serde_json::Value::String(if name.is_empty() { fallback } else { name });
        }
        if let Some(desc) = description {
            pet["description"] = serde_json::Value::String(desc.trim().to_string());
        }

        let new_raw =
            serde_json::to_string_pretty(&pet).map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(&json_path, new_raw).map_err(|e| format!("写入宠物配置失败: {}", e))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("更新任务异常: {}", e))??;

    // 更新成功：通知主进程重建托盘菜单
    let _ = app_for_emit.emit("pet-pets-changed", ());

    Ok(())
}

/// 列出所有已导入的外部宠物（启动时恢复用）。扫描 app_data_dir/pets/，
/// 逐个读取 pet.json + spritesheet.webp，返回宠物定义列表（含 base64）。
#[tauri::command]
pub async fn list_imported_pets(app: tauri::AppHandle) -> Result<Vec<PetDefJson>, String> {
    let pets_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("pets");

    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<PetDefJson>, String> {
        let mut result = Vec::new();
        let entries = match std::fs::read_dir(&pets_root) {
            Ok(e) => e,
            Err(_) => return Ok(result), // 目录不存在 = 还没有导入过，返回空
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let dir = entry.path();
            let pet_json_path = dir.join("pet.json");
            let raw_content = match std::fs::read_to_string(&pet_json_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let raw: RawPetJson = match serde_json::from_str(&raw_content) {
                Ok(r) => r,
                Err(_) => continue,
            };
            // 找 webp（目录下任意 .webp）
            let webp = std::fs::read_dir(&dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .find(|e| {
                    e.path()
                        .extension()
                        .map(|x| x.to_string_lossy().to_lowercase())
                        == Some("webp".into())
                });
            let Some(webp_entry) = webp else { continue };
            let Ok(webp_bytes) = std::fs::read(webp_entry.path()) else {
                continue;
            };
            result.push(build_pet_def(&raw, &webp_bytes));
        }
        Ok(result)
    })
    .await
    .map_err(|e| format!("读取外部宠物列表异常: {e}"))?
}

// ─────────────────────────────────────────────────────────────
// 在线画廊：接入 awesome-codex-pet（GitHub raw 为权威源，codexpet.top 仅预览图）
// ─────────────────────────────────────────────────────────────

/// awesome-codex-pet 仓库（main 分支）原始内容基地址
const CODEPET_GITHUB_RAW: &str =
    "https://raw.githubusercontent.com/legeling/awesome-codex-pet/main";
/// 预览图基地址（codexpet.top 提供，已实测可用；加载失败前端回退文字）
const CODEPET_PREVIEW_BASE: &str = "https://codexpet.top/assets/previews";

/// 建连超时。
const HTTP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// 单次请求总超时。
///
/// 必须设置：`reqwest` 默认**没有**超时，网络挂起时 Promise 永不 resolve，
/// 前端画廊会一直卡在 loading，用户只能杀进程。
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// 共享的 HTTP 客户端（连接复用）。
static HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

/// 获取共享 HTTP 客户端：统一 UA 与超时，并复用连接池。
///
/// 用 OnceLock 而非每次新建，避免每次浏览/下载都重建 TLS 上下文与连接池。
fn http_client() -> Result<&'static reqwest::Client, String> {
    if let Some(c) = HTTP_CLIENT.get() {
        return Ok(c);
    }
    let client = reqwest::Client::builder()
        .user_agent("PetBuddy/0.1 (online-gallery)")
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;
    // 并发下可能已被其它线程写入；插入失败无害，取已有值即可
    let _ = HTTP_CLIENT.set(client);
    HTTP_CLIENT
        .get()
        .ok_or_else(|| "HTTP 客户端未初始化".to_string())
}

/// 在线宠物列表项（画廊用）。来自 awesome-codex-pet 的 pets.json 索引。
#[derive(Serialize, Clone)]
pub struct OnlinePetMeta {
    /// 唯一标识（仓库目录 slug，如 firefly--lingxiaotian），下载时即作为本地 id
    pub slug: String,
    /// 显示名（优先中文 localized_names.zh，回退 name）
    pub name: String,
    pub author: String,
    pub category: String,
    pub description: String,
    /// 精灵图版本（1 或 2），仅展示用，下载时按实际图尺寸修正
    pub sprite_version: u32,
    /// 预览图 URL（codexpet.top 的 idle.webp）；可能 404，前端需容错
    pub preview_url: String,
}

#[derive(serde::Deserialize)]
struct RawOnlinePet {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    localized_names: std::collections::HashMap<String, String>,
    #[serde(default)]
    author: String,
    #[serde(default)]
    primary_category: String,
    #[serde(default)]
    description: String,
    #[serde(default, rename = "spriteVersionNumber")]
    sprite_version_number: u32,
}

/// 浏览在线宠物：拉取 awesome-codex-pet 的 pets.json 索引并返回列表。
#[tauri::command]
pub async fn browse_online_pets() -> Result<Vec<OnlinePetMeta>, String> {
    let url = format!("{CODEPET_GITHUB_RAW}/pets.json");
    let client = http_client()?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("拉取宠物索引失败（网络）: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("拉取宠物索引失败: HTTP {}", resp.status()));
    }
    let list: Vec<RawOnlinePet> = resp
        .json()
        .await
        .map_err(|e| format!("解析宠物索引失败: {e}"))?;

    let mut out = Vec::with_capacity(list.len());
    for p in list {
        if p.slug.is_empty() {
            continue;
        }
        // 显示名：优先中文，回退英文、原始名、slug
        let name = p
            .localized_names
            .get("zh")
            .cloned()
            .filter(|s| !s.is_empty())
            .or_else(|| p.localized_names.get("en").cloned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if p.name.is_empty() {
                    None
                } else {
                    Some(p.name.clone())
                }
            })
            .unwrap_or_else(|| p.slug.clone());
        let preview_url = format!("{CODEPET_PREVIEW_BASE}/{}/webp/idle.webp", p.slug);
        out.push(OnlinePetMeta {
            slug: p.slug,
            name,
            author: p.author,
            category: p.primary_category,
            description: p.description,
            sprite_version: p.sprite_version_number,
            preview_url,
        });
    }
    Ok(out)
}

/// 把 pet.json 的 `id` 统一改写为 slug，返回规范化后的 JSON 文本。
///
/// 纯函数（不碰文件系统、不做网络请求），便于单测。
///
/// 之所以必须改写并**落盘**：本地目录名固定用 slug，而 `delete_imported_pet` /
/// `update_imported_pet` 是用「宠物 id」拼目录路径的。若保留远程 pet.json 里的 id
/// （完全可能与 slug 不同），下次启动 `list_imported_pets` 读到的仍是远程 id，
/// 删除 / 编辑就会去拼一个不存在的目录——而因为代码里有 `if target.exists()` 保护，
/// 表现为**静默成功却什么都没做**。
fn normalize_pet_id_json(json_text: &str, slug: &str) -> Result<String, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| format!("pet.json 解析失败: {e}"))?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "pet.json 根节点不是 JSON 对象".to_string())?;
    obj.insert(
        "id".to_string(),
        serde_json::Value::String(slug.to_string()),
    );
    serde_json::to_string_pretty(&value).map_err(|e| format!("pet.json 序列化失败: {e}"))
}

/// 下载在线宠物：拉取 <slug> 目录下的 pet.json + spritesheet.webp，
/// 复用内置的 build_pet_def 组装，写入 app_data_dir/pets/<slug>/，并刷新托盘菜单。
///
/// 落盘前会把 pet.json 的 id 归一化为 slug（见 normalize_pet_id_json），
/// 保证「宠物 id == 本地目录名」，使后续的删除 / 编辑命令能正确定位目录。
#[tauri::command]
pub async fn download_online_pet(
    app: tauri::AppHandle,
    slug: String,
) -> Result<PetDefJson, String> {
    // id 白名单校验（slug 即本地目录名，防目录穿越）
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("宠物 slug 含非法字符".into());
    }

    let client = http_client()?;

    let json_url = format!("{CODEPET_GITHUB_RAW}/pets/{slug}/pet.json");
    let sheet_url = format!("{CODEPET_GITHUB_RAW}/pets/{slug}/spritesheet.webp");

    let json_resp = client
        .get(&json_url)
        .send()
        .await
        .map_err(|e| format!("拉取 pet.json 失败: {e}"))?;
    if !json_resp.status().is_success() {
        return Err(format!("远程宠物不存在（HTTP {}）", json_resp.status()));
    }
    let json_text = json_resp
        .text()
        .await
        .map_err(|e| format!("读取 pet.json 失败: {e}"))?;

    let sheet_resp = client
        .get(&sheet_url)
        .send()
        .await
        .map_err(|e| format!("拉取 spritesheet 失败: {e}"))?;
    if !sheet_resp.status().is_success() {
        return Err(format!("远程精灵图不存在（HTTP {}）", sheet_resp.status()));
    }
    let sheet_bytes = sheet_resp
        .bytes()
        .await
        .map_err(|e| format!("读取精灵图字节失败: {e}"))?;

    // 归一化 id 为 slug，并**写回磁盘**（见 normalize_pet_id_json 的说明）。
    // 修复前只改内存中的 raw、落盘的仍是原文，导致重启后 id 又变回远程值。
    let json_text = normalize_pet_id_json(&json_text, &slug)?;

    let raw: RawPetJson =
        serde_json::from_str(&json_text).map_err(|e| format!("pet.json 解析失败: {e}"))?;

    // 组装定义并落盘（含把 webp 写入目录，便于后续 list_imported_pets 复用）
    let pets_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("pets");
    let target_dir = pets_root.join(&slug);
    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir).map_err(|e| format!("清理旧宠物目录失败: {e}"))?;
    }
    std::fs::create_dir_all(&target_dir).map_err(|e| format!("创建宠物目录失败: {e}"))?;
    std::fs::write(target_dir.join("pet.json"), &json_text)
        .map_err(|e| format!("写入 pet.json 失败: {e}"))?;
    std::fs::write(target_dir.join("spritesheet.webp"), &sheet_bytes)
        .map_err(|e| format!("写入 spritesheet 失败: {e}"))?;

    let def = build_pet_def(&raw, &sheet_bytes);

    // 下载成功：通知主进程重建托盘菜单
    let _ = app.emit("pet-pets-changed", ());

    Ok(def)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_pet_id_json ──
    // 覆盖 P0-2 的回归：目录名用 slug、id 用远程 pet.json 的 id，
    // 二者不同时删除 / 编辑会静默失效。

    fn id_of(json: &str) -> String {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    #[test]
    fn normalize_replaces_upstream_id_with_slug() {
        let out = normalize_pet_id_json(
            r#"{"id":"upstream-name","displayName":"X","description":"d"}"#,
            "the--slug",
        )
        .unwrap();

        assert_eq!(id_of(&out), "the--slug");
        // 其余字段必须原样保留
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["displayName"], "X");
        assert_eq!(v["description"], "d");
    }

    #[test]
    fn normalize_fills_empty_id() {
        let out = normalize_pet_id_json(r#"{"id":"","displayName":"Y"}"#, "s1").unwrap();
        assert_eq!(id_of(&out), "s1");
    }

    #[test]
    fn normalize_adds_missing_id() {
        let out = normalize_pet_id_json(r#"{"displayName":"Z"}"#, "s2").unwrap();
        assert_eq!(id_of(&out), "s2");

        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["displayName"], "Z");
    }

    #[test]
    fn normalize_preserves_unknown_and_nested_fields() {
        let out = normalize_pet_id_json(r#"{"id":"a","custom":{"nested":[1,2]}}"#, "s3").unwrap();

        assert_eq!(id_of(&out), "s3");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["custom"]["nested"][1], 2);
    }

    #[test]
    fn normalize_rejects_invalid_json() {
        assert!(normalize_pet_id_json("not json", "s").is_err());
        assert!(normalize_pet_id_json("", "s").is_err());
    }

    #[test]
    fn normalize_rejects_non_object_root() {
        assert!(normalize_pet_id_json("[1,2,3]", "s").is_err());
        assert!(normalize_pet_id_json("\"str\"", "s").is_err());
        assert!(normalize_pet_id_json("42", "s").is_err());
    }

    #[test]
    fn normalized_json_is_parseable_as_raw_pet_json() {
        // 关键回归：落盘的文本必须能被 RawPetJson 解析，
        // 否则重启后 list_imported_pets 会静默跳过这只宠物。
        let out =
            normalize_pet_id_json(r#"{"id":"upstream","displayName":"N"}"#, "the-slug").unwrap();

        let raw: RawPetJson = serde_json::from_str(&out).unwrap();
        assert_eq!(raw.id, "the-slug");
        assert_eq!(raw.display_name.as_deref(), Some("N"));
    }

    #[test]
    fn normalized_id_matches_local_dir_name_contract() {
        // 「id == 目录名」这条契约由 delete_imported_pet / update_imported_pet 依赖，
        // 这里把契约显式断言下来，避免将来有人改回「用远程 id」。
        let slug = "firefly--lingxiaotian";
        let out = normalize_pet_id_json(r#"{"id":"totally-different"}"#, slug).unwrap();

        let raw: RawPetJson = serde_json::from_str(&out).unwrap();
        let pets_root = std::path::Path::new("/tmp/pets");
        assert_eq!(pets_root.join(&raw.id), pets_root.join(slug));
    }

    // ── http_client ──

    #[test]
    fn http_client_is_reused_across_calls() {
        // 验证 OnceLock 生效：不是每次调用都新建客户端（否则连接池形同虚设）。
        // 超时配置无法从 reqwest::Client 读回断言，故只验证复用这一条可观测性质。
        let a = http_client().unwrap();
        let b = http_client().unwrap();
        assert!(std::ptr::eq(a, b));
    }

    // ── 外部宠物 per-pet frame 支持（P0-1）──

    /// 构造一个尺寸为 (w, h) 的最小有效 VP8X webp（仅含头部，不含像素），
    /// 供 build_pet_def 解析实际尺寸用。
    ///
    /// 注意：与 webp_dimensions 的实际解析偏移对齐——该函数从 data[20] 起读
    /// 24 位宽/高（data[16..20] 是它不读的 chunk size 字段），故 width-1 放
    /// data[20..23]、height-1 放 data[23..26]。
    fn make_webp(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(22u32).to_le_bytes());
        v.extend_from_slice(b"WEBP");
        v.extend_from_slice(b"VP8X");
        v.extend_from_slice(&10u32.to_le_bytes()); // data[16..20] chunk size（webp_dims 不读）
        let w1 = (w - 1).to_le_bytes();
        let h1 = (h - 1).to_le_bytes();
        v.push(w1[0]);
        v.push(w1[1]);
        v.push(w1[2]); // data[20..23] = width-1
        v.push(h1[0]);
        v.push(h1[1]);
        v.push(h1[2]); // data[23..26] = height-1
        v.extend_from_slice(&[0u8; 4]); // 补足到 30 字节（满足 data.len() < 30 检查）
        v
    }

    fn raw_with_frame(id: &str, frame: Option<RawFrame>) -> RawPetJson {
        RawPetJson {
            id: id.to_string(),
            display_name: None,
            description: None,
            idle: None,
            talk: None,
            actions: None,
            frame,
        }
    }

    #[test]
    fn compute_frame_standard_192x208_8cols() {
        // 验收用例 1：标准包，sheet 1536×2288（8 列 × 11 行）
        let raw = raw_with_frame(
            "std",
            Some(RawFrame {
                width: 192,
                height: 208,
                cols: 8,
            }),
        );
        let f = compute_frame(&raw, Some((1536, 2288)));
        assert_eq!(
            f,
            PetFrameJson {
                width: 192,
                height: 208,
                cols: 8,
                rows: 11
            }
        );
    }

    #[test]
    fn compute_frame_nonstandard_256x256_6cols() {
        // 验收用例 2：非标包，sheet 1536×2816（6 列 × 11 行）
        let raw = raw_with_frame(
            "hd",
            Some(RawFrame {
                width: 256,
                height: 256,
                cols: 6,
            }),
        );
        let f = compute_frame(&raw, Some((1536, 2816)));
        assert_eq!(
            f,
            PetFrameJson {
                width: 256,
                height: 256,
                cols: 6,
                rows: 11
            }
        );
    }

    #[test]
    fn compute_frame_declares_width_zero_falls_back_to_sheet() {
        // 声明宽度为 0（非法）→ 用 sheet 宽 / cols 反推单帧宽
        let raw = raw_with_frame(
            "x",
            Some(RawFrame {
                width: 0,
                height: 200,
                cols: 5,
            }),
        );
        let f = compute_frame(&raw, Some((1000, 2000)));
        assert_eq!(f.width, 200); // 1000 / 5
        assert_eq!(f.height, 200);
        assert_eq!(f.cols, 5);
        assert_eq!(f.rows, 10); // 2000 / 200
    }

    #[test]
    fn compute_frame_undeclared_derives_from_sheet() {
        // 完全不声明 frame → 默认 192×208/8 列，rows 由 sheet 高 / 208 得出
        let raw = raw_with_frame("def", None);
        let f = compute_frame(&raw, Some((1536, 2288)));
        assert_eq!(f.width, 192); // 1536 / 8
        assert_eq!(f.height, 208);
        assert_eq!(f.cols, 8);
        assert_eq!(f.rows, 11);
    }

    #[test]
    fn clamp_seq_uses_real_cols_not_hardcoded() {
        // 17 列高清包：count 上限应为 17，而非写死的 8（回归 P0-1 关键）
        let s = seq(0, 20, 10);
        let c = clamp_seq(s, 11, 17).unwrap();
        assert_eq!(c.count, 17);

        // 6 列包：count 超过 6 应被截断到 6
        let c2 = clamp_seq(seq(0, 9, 10), 11, 6).unwrap();
        assert_eq!(c2.count, 6);

        // 行越界 → None
        assert!(clamp_seq(seq(99, 4, 10), 11, 8).is_none());
    }

    #[test]
    fn build_pet_def_emits_per_pet_frame() {
        // 端到端：build_pet_def 把 per-pet frame 写进 PetDefJson.frame
        let raw = raw_with_frame(
            "e2e",
            Some(RawFrame {
                width: 256,
                height: 256,
                cols: 6,
            }),
        );
        let sheet = make_webp(1536, 2816);
        let def = build_pet_def(&raw, &sheet);
        assert_eq!(
            def.frame,
            PetFrameJson {
                width: 256,
                height: 256,
                cols: 6,
                rows: 11
            }
        );
        // 默认 idle 的 count 应受真实列数限制（6），而非 8
        assert_eq!(def.idle.count, 6);
    }
}
