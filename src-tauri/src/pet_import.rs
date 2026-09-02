// 外部宠物导入（装配 / IO 层）：把用户选择的 .zip 解压到 app_data_dir/pets/<id>/，
// 读取其中的 pet.json（Codex 格式），套用标准精灵图布局，返回宠物定义给前端。
//
// 纯逻辑（帧几何计算、webp 尺寸解析、base64、id 校验、越界修正）已迁入
// `domain::pet`，本文件只做平台 IO：解压、读写磁盘、网络、拼目录路径。
//
// zip 结构约定（与内置宠物包一致）：
//   pet.json           # 必填：{ id, displayName, description, ... }
//   spritesheet.webp   # 必填：精灵图
//
// 帧布局默认套用 Codex Pet V2 标准（与 manifest.json 内置宠物一致）：
//   idle=row0/6帧；talk=row3/4帧；
//   actions: wave/row3、jump/row4、failed/row5、waiting/row6、look/row9。
// 若 pet.json 额外提供 idle / talk / actions 字段则覆盖默认值。
//
// spritesheet 以 base64 data URL 返回（避免引入 asset 协议配置），前端直接可用。

use std::io::{self, Read};
use tauri::{Emitter, Manager};

use crate::domain::gallery::index::{
    map_online_pets, pet_json_url, spritesheet_url, OnlinePetMeta, RawOnlinePet,
};
use crate::domain::pet::codec::base64_decode;
use crate::domain::pet::model::{build_pet_def, PetDefJson, RawPetJson};
use crate::domain::pet::validator::safe_join;
use crate::infra::http_client::http_client;

/// 单条解压条目（未压缩）的硬上限：50 MB。
///
/// 防「解压炸弹」：zip 头里的 `entry.size()` 是攻击者可控的声明值，
/// 不可直接 `Vec::with_capacity(entry.size())`（会瞬间预约数 GB 内存），
/// 更不能 `read_to_end`（声明 4GB 的条目会把内存撑爆）。
/// 这里改为分块读取并强制总量上限，超限即报错。
/// 正常宠物精灵图仅数百 KB～数 MB，50 MB 是宽松上限。
const MAX_PET_FILE_BYTES: usize = 50 * 1024 * 1024;

/// 同上，但针对文本类（pet.json），限制更紧（1 MB）。
const MAX_PET_JSON_BYTES: usize = 1024 * 1024;

/// 分块读取 `Read`，总量不超过 `max` 字节（防 zip 炸弹）。
///
/// 不信任 `entry.size()`，只按实际读到的字节累加判断；超出上限立即返回错误，
/// 避免内存被恶意声明撑爆。返回读到的全部字节。
fn read_entry_bounded<R: Read>(mut reader: R, max: usize) -> io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(max.min(64 * 1024));
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        if buf.len() + n > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("解压条目超出大小上限（{max} 字节）"),
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
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
                let bytes = read_entry_bounded(
                    zip.by_index(i).map_err(|e| format!("zip 读取失败: {e}"))?,
                    MAX_PET_JSON_BYTES,
                )
                .map_err(|e| format!("pet.json 读取失败: {e}"))?;
                let content =
                    String::from_utf8(bytes).map_err(|e| format!("pet.json 编码非 UTF-8: {e}"))?;
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
        // id 校验（防目录穿越）→ 拼出宠物目录
        let target_dir = safe_join(&root, &id).map_err(|e| e.message)?;

        // 先解压到临时目录，成功后再原子改名覆盖旧目录：
        // 这样中途失败（解压炸弹 / 缺文件 / IO 错误）不会留下「半个损坏宠物」，
        // 已存在的旧宠物目录保持完整。
        let tmp_dir = root.join(format!(".import-tmp-{id}"));
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir).map_err(|e| format!("清理临时目录失败: {e}"))?;
        }
        std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建宠物目录失败: {e}"))?;

        let mut webp_bytes: Option<Vec<u8>> = None;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| format!("zip 读取失败: {e}"))?;
            let Some(rel) = entry.enclosed_name() else {
                continue;
            };
            let lower = rel.to_string_lossy().to_lowercase();
            let out = tmp_dir.join(&rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out).map_err(|e| format!("解压建目录失败: {e}"))?;
            } else {
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("解压建目录失败: {e}"))?;
                }
                // 分块读取并强制大小上限：不信任 entry.size()，防解压炸弹。
                let buf = read_entry_bounded(&mut entry, MAX_PET_FILE_BYTES)
                    .map_err(|e| format!("解压读取失败: {e}"))?;
                std::fs::write(&out, &buf).map_err(|e| format!("解压写文件失败: {e}"))?;
                // 精灵图选取：优先精确名 spritesheet.webp（覆盖已有）；
                // 否则兜底取第一个 .webp（避免「取 zip 里最后一个 .webp」的随机性）。
                let name_lower = entry.name().to_string().to_lowercase();
                if lower.ends_with(".webp")
                    && (name_lower.ends_with("spritesheet.webp") || webp_bytes.is_none())
                {
                    webp_bytes = Some(buf);
                }
            }
        }

        let webp_bytes = match webp_bytes {
            Some(b) => b,
            None => {
                // 缺精灵图：清理临时目录，避免残留空目录；旧宠物目录保持不变。
                let _ = std::fs::remove_dir_all(&tmp_dir);
                return Err("压缩包内未找到 spritesheet.webp".into());
            }
        };

        // 全部就绪：原子替换旧目录（先删旧、再改名；同父目录下 rename 为 O(1)）。
        if target_dir.exists() {
            std::fs::remove_dir_all(&target_dir).map_err(|e| format!("清理旧宠物目录失败: {e}"))?;
        }
        std::fs::rename(&tmp_dir, &target_dir).map_err(|e| format!("写入宠物目录失败: {e}"))?;

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
    let target = safe_join(&pets_root, &id).map_err(|e| e.message)?;
    let app_for_emit = app.clone();

    let _ = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
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
    let pets_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("pets");
    let target = safe_join(&pets_root, &id).map_err(|e| e.message)?;
    let app_for_emit = app.clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let json_path = target.join("pet.json");
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

/// 浏览在线宠物：拉取 awesome-codex-pet 的 pets.json 索引并返回列表。
#[tauri::command]
pub async fn browse_online_pets() -> Result<Vec<OnlinePetMeta>, String> {
    let url = format!(
        "{}/pets.json",
        crate::domain::gallery::index::CODEPET_GITHUB_RAW
    );
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

    Ok(map_online_pets(list))
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
    let pets_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("pets");
    let target_dir = safe_join(&pets_root, &slug).map_err(|e| e.message)?;

    let client = http_client()?;

    let json_url = pet_json_url(&slug);
    let sheet_url = spritesheet_url(&slug);

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

    // ── read_entry_bounded（解压炸弹防护）──
    // 回归 P0 的 `Vec::with_capacity(entry.size())` 信任攻击者声明大小，
    // 以及 read_to_end 实际撑爆内存的漏洞。

    #[test]
    fn read_entry_bounded_accepts_small_payload() {
        let data = vec![1u8, 2, 3, 4];
        let out = read_entry_bounded(&data[..], 1024).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn read_entry_bounded_rejects_oversized_stream() {
        // 模拟一个持续吐数据、实际远超上限的流：必须报错而非分配数 GB。
        let big = std::io::repeat(0u8).take((MAX_PET_FILE_BYTES as u64) + 1);
        let err = read_entry_bounded(big, MAX_PET_FILE_BYTES).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_entry_bounded_exact_limit_is_ok() {
        // 恰好等于上限的流应成功（边界值不被误拒）。
        let data = std::io::repeat(7u8).take(MAX_PET_FILE_BYTES as u64);
        let out = read_entry_bounded(data, MAX_PET_FILE_BYTES).unwrap();
        assert_eq!(out.len(), MAX_PET_FILE_BYTES);
    }
}
