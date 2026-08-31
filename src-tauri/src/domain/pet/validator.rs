//! 外部宠物 id 校验、帧段越界修正、路径穿越检查（纯函数）。

use std::path::Path;

use crate::domain::pet::model::FrameSeqJson;
use crate::error::AppError;

/// 宠物 id 白名单：仅允许 ASCII 字母数字、下划线、连字符，且非空。
///
/// 这是所有外部宠物文件系统操作的入口门禁：id 来自不受信任的 pet.json，
/// 必须先用此函数过一道，再拼路径，杜绝任意路径穿越。
pub fn is_valid_pet_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 把 root 与 id 拼接为子路径，并校验 id 合法（且结果不会逃逸出 root）。
///
/// 白名单已排除 `/` `\` `.` `..`，所以 `root.join(id)` 在语法上不可能逃逸；
/// 这里再兜底做一次前缀检查，满足「目录穿越检查」需求（对应计划 2.5 / 4.4）。
pub fn safe_join(root: &Path, id: &str) -> Result<std::path::PathBuf, AppError> {
    if !is_valid_pet_id(id) {
        return Err(AppError::invalid_pet_id(id));
    }
    let joined = root.join(id);
    if !joined.starts_with(root) {
        return Err(AppError::zip_slip(&joined));
    }
    Ok(joined)
}

/// 根据精灵图实际尺寸，修正一个帧段的 row/count，避免越界：
///
///   - row 超出实际行数 → 返回 None（该动作不可用，应移除）
///   - count / fps 为 0 → 用兜底值（0 帧或 0 fps 会导致动画卡死/不播放）
///   - count 超出该行剩余列数 → 截断到可用列数（用该宠物的真实列数，
///     而非写死的 FRAME_COLS，否则 17 列高清包的帧会被错误截断）
pub fn clamp_seq(s: FrameSeqJson, rows: u32, cols: u32) -> Option<FrameSeqJson> {
    if s.row >= rows {
        return None;
    }
    let count = if s.count == 0 {
        cols
    } else {
        s.count.min(cols)
    };
    let fps = if s.fps == 0 { 8 } else { s.fps };
    Some(FrameSeqJson {
        row: s.row,
        count,
        fps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_pet_id_rules() {
        assert!(is_valid_pet_id("miku"));
        assert!(is_valid_pet_id("ryujin-maru"));
        assert!(is_valid_pet_id("Pet_01"));
        assert!(!is_valid_pet_id(""));
        assert!(!is_valid_pet_id("a/b"));
        assert!(!is_valid_pet_id("../etc"));
        assert!(!is_valid_pet_id("a..b"));
        assert!(!is_valid_pet_id("中文"));
        assert!(!is_valid_pet_id("a b"));
    }

    #[test]
    fn safe_join_blocks_traversal() {
        let root = std::path::Path::new("/data/pets");
        // 合法 id → 正常拼接
        assert_eq!(
            safe_join(root, "miku").unwrap(),
            std::path::Path::new("/data/pets/miku")
        );
        // 非法 id（含 ..）→ 拒绝
        assert!(safe_join(root, "../etc").is_err());
        assert!(safe_join(root, "").is_err());
        // 结果确实在 root 下
        assert!(safe_join(root, "miku").unwrap().starts_with(root));
    }

    #[test]
    fn clamp_seq_uses_real_cols() {
        // 17 列高清包：count=20 应被真实列数 17 截断，而非写死的 8
        let s = FrameSeqJson {
            row: 0,
            count: 20,
            fps: 12,
        };
        let r = clamp_seq(s.clone(), 11, 17).unwrap();
        assert_eq!(r.count, 17);
        assert_eq!(r.fps, 12);

        // row 越界 → 移除（rows=1 时只有 row 0 合法）
        assert!(clamp_seq(FrameSeqJson { row: 99, count: 4, fps: 10 }, 1, 17).is_none());

        // fps=0 → 兜底 8
        let s0 = FrameSeqJson {
            row: 0,
            count: 5,
            fps: 0,
        };
        assert_eq!(clamp_seq(s0, 11, 8).unwrap().fps, 8);

        // count=0 → 兜底为整行列数
        let s1 = FrameSeqJson {
            row: 0,
            count: 0,
            fps: 8,
        };
        assert_eq!(clamp_seq(s1, 11, 8).unwrap().count, 8);
    }
}
