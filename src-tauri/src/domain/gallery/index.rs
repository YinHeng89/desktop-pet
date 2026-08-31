// 在线宠物索引的纯映射与 URL 构造。
//
// `map_online_pets` 把远端 `pets.json` 的原始条目（RawOnlinePet）映射为前端用的
// `OnlinePetMeta`，含「显示名回退链」；`preview_url` / `pet_json_url` / `spritesheet_url`
// 按 slug 拼出 codexpet 仓库/预览站的资源 URL。全部无 IO。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// awesome-codex-pet 仓库原始索引（pets.json）的 GitHub raw 基址。
pub const CODEPET_GITHUB_RAW: &str = "https://raw.githubusercontent.com/legeling/awesome-codex-pet/main";
/// codexpet.top 预览图基址。
pub const CODEPET_PREVIEW_BASE: &str = "https://codexpet.top/assets/previews";

/// 在线宠物列表项（画廊用），返回给前端。
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

/// 远端 `pets.json` 索引的原始条目（反序列化目标）。
#[derive(Deserialize)]
pub struct RawOnlinePet {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub localized_names: HashMap<String, String>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub primary_category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "spriteVersionNumber")]
    pub sprite_version_number: u32,
}

/// 预览图 URL（codexpet.top 的 idle.webp）。
pub fn preview_url(slug: &str) -> String {
    format!("{CODEPET_PREVIEW_BASE}/{slug}/webp/idle.webp")
}

/// 远程宠物 pet.json 下载地址。
pub fn pet_json_url(slug: &str) -> String {
    format!("{CODEPET_GITHUB_RAW}/pets/{slug}/pet.json")
}

/// 远程宠物 spritesheet.webp 下载地址。
pub fn spritesheet_url(slug: &str) -> String {
    format!("{CODEPET_GITHUB_RAW}/pets/{slug}/spritesheet.webp")
}

/// 把远端索引条目映射为画廊展示项。
///
/// 显示名回退链：`zh` → `en` → `name` → `slug`（任一为空则跳到下一级）。
/// `slug` 为空的条目跳过（无法作为本地目录名）。
pub fn map_online_pets(raw: Vec<RawOnlinePet>) -> Vec<OnlinePetMeta> {
    let mut out = Vec::with_capacity(raw.len());
    for p in raw {
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
        let preview = preview_url(&p.slug);
        out.push(OnlinePetMeta {
            slug: p.slug,
            name,
            author: p.author,
            category: p.primary_category,
            description: p.description,
            sprite_version: p.sprite_version_number,
            preview_url: preview,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> RawOnlinePet {
        RawOnlinePet {
            slug: "firefly--ling".to_string(),
            name: String::new(),
            localized_names: HashMap::new(),
            author: "a".to_string(),
            primary_category: "c".to_string(),
            description: "d".to_string(),
            sprite_version_number: 2,
        }
    }

    // ── 显示名回退链 ──
    #[test]
    fn name_falls_back_zh_then_en() {
        let mut r = raw();
        r.localized_names.insert("zh".into(), "中文名".into());
        r.localized_names.insert("en".into(), "English".into());
        let m = map_online_pets(vec![r]);
        assert_eq!(m[0].name, "中文名");
    }

    #[test]
    fn name_falls_back_to_en_when_zh_empty() {
        let mut r = raw();
        r.localized_names.insert("zh".into(), String::new());
        r.localized_names.insert("en".into(), "English".into());
        let m = map_online_pets(vec![r]);
        assert_eq!(m[0].name, "English");
    }

    #[test]
    fn name_falls_back_to_original_name() {
        let mut r = raw();
        r.name = "RawName".into();
        // zh/en 都缺
        let m = map_online_pets(vec![r]);
        assert_eq!(m[0].name, "RawName");
    }

    #[test]
    fn name_falls_back_to_slug_when_all_empty() {
        let r = raw(); // name 空、localized_names 空
        let m = map_online_pets(vec![r]);
        assert_eq!(m[0].name, "firefly--ling");
    }

    #[test]
    fn empty_slug_is_skipped() {
        let mut r = raw();
        r.slug = String::new();
        assert!(map_online_pets(vec![r]).is_empty());
    }

    #[test]
    fn maps_all_fields() {
        let r = raw();
        let m = &map_online_pets(vec![r])[0];
        assert_eq!(m.author, "a");
        assert_eq!(m.category, "c");
        assert_eq!(m.description, "d");
        assert_eq!(m.sprite_version, 2);
        assert_eq!(m.preview_url, preview_url(&m.slug));
    }

    // ── URL 构造 ──
    #[test]
    fn urls_are_well_formed() {
        assert_eq!(
            preview_url("s--x"),
            "https://codexpet.top/assets/previews/s--x/webp/idle.webp"
        );
        assert_eq!(
            pet_json_url("s--x"),
            "https://raw.githubusercontent.com/legeling/awesome-codex-pet/main/pets/s--x/pet.json"
        );
        assert_eq!(
            spritesheet_url("s--x"),
            "https://raw.githubusercontent.com/legeling/awesome-codex-pet/main/pets/s--x/spritesheet.webp"
        );
    }
}
