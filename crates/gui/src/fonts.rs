//! CJK 字体：egui 默认字体无中文，按 OS 候选表找系统字库。
//!
//! skrifa 天然支持 `.ttc` 合集：逐分面探测含 '中' 字形的首个可用分面，
//! 不硬编码下标（PingFang.ttc 这类多分面文件各系统排序不一）。

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use skrifa::MetadataProvider as _;

pub const CJK_FONT_NAME: &str = "hangar-cjk";

/// 候选字库文件（顺序即优先级），分面下标靠探测确定
fn candidate_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    #[cfg(target_os = "windows")]
    {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        for name in ["msyh.ttc", "simhei.ttf", "simsun.ttc"] {
            v.push(PathBuf::from(&windir).join("Fonts").join(name));
        }
    }
    #[cfg(target_os = "macos")]
    {
        v.push(PathBuf::from("/System/Library/Fonts/PingFang.ttc"));
        v.push(PathBuf::from(
            "/System/Library/Fonts/Supplemental/Songti.ttc",
        ));
        v.push(PathBuf::from("/System/Library/Fonts/STHeiti Light.ttc"));
        v.push(PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"));
    }
    #[cfg(target_os = "linux")]
    {
        v.push(PathBuf::from(
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        ));
        v.push(PathBuf::from(
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ));
        v.push(PathBuf::from(
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ));
        v.push(PathBuf::from("/usr/share/fonts/truetype/arphic/uming.ttc"));
    }
    v
}

/// 某分面是否含中文（'中' U+4E2D 可映射即算）
fn face_has_cjk(bytes: &[u8], index: u32) -> bool {
    // 单文件限 200MB，防误读超大文件
    if bytes.len() > 200 * 1024 * 1024 {
        return false;
    }
    let Ok(font) = skrifa::FontRef::from_index(bytes, index) else {
        return false;
    };
    font.charmap().map('中').is_some()
}

/// 首个“存在且某分面含中文”的 (字节, 分面序号)
fn first_usable(candidates: &[PathBuf]) -> Option<(Vec<u8>, u32)> {
    for path in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // 分面逐个试，上限 32 兜底（序号越界即 from_index 失败，自动跳过）
        for index in 0..32 {
            if face_has_cjk(&bytes, index) {
                return Some((bytes, index));
            }
        }
    }
    None
}

/// 安装 CJK 回退字体（追加到 proportional 末尾：中文缺字时兜底，西文仍用默认字体）。
/// 返回命中的字库路径（供状态行展示），无命中返回 None（界面中文显示方框）。
pub fn install_cjk(ctx: &egui::Context) -> Option<String> {
    let paths = candidate_files();
    let (bytes, index) = first_usable(&paths)?;
    let hit = paths
        .iter()
        .find(|p| std::fs::metadata(p).is_ok())
        .map(|p| p.display().to_string());
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        CJK_FONT_NAME.to_string(),
        Arc::new(egui::FontData {
            font: Cow::Owned(bytes),
            index,
            tweak: Default::default(),
        }),
    );
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        if !family.iter().any(|f| f == CJK_FONT_NAME) {
            family.push(CJK_FONT_NAME.to_string());
        }
    }
    ctx.set_fonts(fonts);
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_non_empty() {
        assert!(!candidate_files().is_empty());
    }

    #[test]
    fn probe_rejects_garbage() {
        assert!(!face_has_cjk(b"not a font at all", 0));
        assert!(!face_has_cjk(&[], 0));
        assert!(first_usable(&[PathBuf::from("/nonexistent/font.ttc")]).is_none());
    }
}
