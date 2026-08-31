//! 外部宠物的数据模型与构建逻辑（纯函数，零 Tauri / 零 IO）。
//!
//! 从原 pet_import.rs 迁出，行为完全等价（仅类型改为 pub、移至 domain 层）。

use serde::Serialize;
use std::collections::BTreeMap;

use crate::domain::pet::codec::{base64_encode, webp_dimensions};
use crate::domain::pet::validator::clamp_seq;

/// 一个动作/状态（idle/talk/某 action）对应的「第几行的连续 count 帧，每秒 fps 帧」。
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct FrameSeqJson {
    pub row: u32,
    pub count: u32,
    pub fps: u32,
}

/// 单帧几何（像素 + 行列数）。外部宠物可能非标准（256×256、6 列等），
/// 这里把它的真实几何显式带上，前端据此切帧，避免用全局写死的 192×208/8 列错位。
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct PetFrameJson {
    pub width: u32,
    pub height: u32,
    pub cols: u32,
    pub rows: u32,
}

/// 构建后的宠物定义（返回给前端）。
#[derive(Serialize, Clone)]
pub struct PetDefJson {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub spritesheet: String,
    pub idle: FrameSeqJson,
    pub talk: FrameSeqJson,
    pub actions: BTreeMap<String, FrameSeqJson>,
    /// 该宠物精灵图的真实单帧几何，前端按它切帧。
    pub frame: PetFrameJson,
}

/// 未处理前的原始 pet.json（来自前端或 zip）。
#[derive(serde::Deserialize, Clone)]
pub struct RawPetJson {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub idle: Option<RawSeq>,
    #[serde(default)]
    pub talk: Option<RawSeq>,
    #[serde(default)]
    pub actions: Option<BTreeMap<String, RawSeq>>,
    /// 可选的帧几何声明。缺失时由精灵图实际尺寸回退推导（见 compute_frame）。
    #[serde(default)]
    pub frame: Option<RawFrame>,
}

/// 外部 pet.json 里的帧几何声明（可选）。
#[derive(serde::Deserialize, Clone)]
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub cols: u32,
}

#[derive(serde::Deserialize, Clone)]
pub struct RawSeq {
    pub row: u32,
    pub count: u32,
    pub fps: u32,
}

/// 构造一个动作帧段。
pub fn seq(row: u32, count: u32, fps: u32) -> FrameSeqJson {
    FrameSeqJson { row, count, fps }
}

/// Rust 侧用于 row/count 越界估算的「默认假设」帧尺寸。
/// 注意：这并非强制标准。项目支持任意帧尺寸的外部包(Codex 生成的高清大帧
/// 尺寸远大于此、列数也非 8)，前端按 manifest 声明的真实 frame 尺寸自适应取帧。
/// 这里仅用于保守估算行数、以及 clamp_seq 对每行列数做上限保护。
const FRAME_H: u32 = 208;
const FRAME_COLS: u32 = 8;

/// 标准动作集合（兜底用，覆盖 Codex Pet V2 的常用动作）。
fn default_actions() -> BTreeMap<String, FrameSeqJson> {
    let mut m = BTreeMap::new();
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
pub fn build_pet_def(raw: &RawPetJson, spritesheet_bytes: &[u8]) -> PetDefJson {
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
    let actions: BTreeMap<String, FrameSeqJson> = actions_raw
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个尺寸为 (w, h) 的最小有效 VP8X webp（仅含头部，不含像素），
    /// 供 build_pet_def 解析实际尺寸用。
    ///
    /// 注意：与 webp_dimensions 的实际解析偏移对齐——payload 从 data[20] 起
    /// （已过 4 字节 chunk size），VP8X 的 24 位宽-1 在 payload[4..7]、高-1 在
    /// payload[7..10]。故 width-1 放 data[24..27]、height-1 放 data[27..30]。
    fn make_webp(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(22u32).to_le_bytes());
        v.extend_from_slice(b"WEBP");
        v.extend_from_slice(b"VP8X");
        v.extend_from_slice(&10u32.to_le_bytes()); // data[16..20] chunk size
        v.extend_from_slice(&[0u8; 4]); // data[20..24] = flags（payload[0..4]）
        let w1 = (w - 1).to_le_bytes();
        let h1 = (h - 1).to_le_bytes();
        v.push(w1[0]);
        v.push(w1[1]);
        v.push(w1[2]); // data[24..27] = width-1（payload[4..7]）
        v.push(h1[0]);
        v.push(h1[1]);
        v.push(h1[2]); // data[27..30] = height-1（payload[7..10]）
        // 总长 30 字节，恰好满足 data.len() < 30 的边界检查
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
        let raw = raw_with_frame("def", None);
        let f = compute_frame(&raw, Some((1536, 2288)));
        assert_eq!(f.width, 192); // 1536 / 8
        assert_eq!(f.height, 208);
        assert_eq!(f.cols, 8);
        assert_eq!(f.rows, 11);
    }

    #[test]
    fn build_pet_def_emits_per_pet_frame() {
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
