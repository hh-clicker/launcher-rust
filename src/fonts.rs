//! Losslessly packed original fonts, mapped read-only instead of copied into heap buffers.
use anyhow::{bail, Context, Result};
use eframe::egui::{FontData, FontDefinitions, FontFamily, FontTweak};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs::File, os::windows::fs::OpenOptionsExt, path::Path, sync::OnceLock};

const INDEX: &str = include_str!(concat!(env!("OUT_DIR"), "/font-index.json"));
#[derive(Deserialize)]
struct Index {
    bytes: usize,
    sha256: String,
    fonts: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    name: String,
    offset: usize,
    bytes: usize,
    sha256: String,
    scale: f32,
}
struct Storage {
    // Keep the non-write/non-delete-sharing file handle alive for the whole mapping.
    _file: File,
    map: memmap2::Mmap,
    index: Index,
}
static STORAGE: OnceLock<Storage> = OnceLock::new();

impl Storage {
    fn open(path: &Path) -> Result<Self> {
        let index: Index = serde_json::from_str(INDEX)?;
        let file = File::options().read(true).share_mode(1).open(path)?;
        if file.metadata()?.len() != index.bytes as u64 {
            bail!("Повреждён размер fonts.dat");
        }
        // The open handle denies writes/deletion; all bytes are verified against
        // the index compiled into this executable before exposing font slices.
        let map = unsafe { memmap2::MmapOptions::new().map(&file)? };
        if format!("{:x}", Sha256::digest(&map)) != index.sha256 {
            bail!("Повреждены шрифты приложения");
        }
        for entry in &index.fonts {
            let end = entry
                .offset
                .checked_add(entry.bytes)
                .context("Font range overflow")?;
            let bytes = map
                .get(entry.offset..end)
                .context("Font range outside fonts.dat")?;
            if format!("{:x}", Sha256::digest(bytes)) != entry.sha256 {
                bail!("Повреждён шрифт {}", entry.name);
            }
        }
        Ok(Self {
            _file: file,
            map,
            index,
        })
    }
}

pub fn definitions(path: &Path) -> Result<FontDefinitions> {
    if STORAGE.get().is_none() {
        let _ = STORAGE.set(Storage::open(path)?);
    }
    let storage: &'static Storage = STORAGE.get().context("Fonts not initialized")?;
    let mut fonts = FontDefinitions::empty();
    for entry in &storage.index.fonts {
        let bytes: &'static [u8] = &storage.map[entry.offset..entry.offset + entry.bytes];
        fonts.font_data.insert(
            entry.name.clone(),
            FontData::from_static(bytes)
                .tweak(FontTweak {
                    scale: entry.scale,
                    ..Default::default()
                })
                .into(),
        );
    }
    fonts.families.insert(
        FontFamily::Monospace,
        vec!["Inter".into(), "NotoEmoji-Regular".into()],
    );
    fonts.families.insert(
        FontFamily::Proportional,
        vec!["Inter".into(), "NotoEmoji-Regular".into()],
    );
    fonts.families.insert(
        FontFamily::Name("Inter Bold".into()),
        vec!["Inter Bold".into()],
    );
    Ok(fonts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_font_bytes_and_fallbacks_match_original_assets() {
        let temporary = std::env::temp_dir().join(format!("hh-font-test-{}", uuid::Uuid::new_v4()));
        let payload = crate::bundle::ensure(&temporary).unwrap();
        let bytes = std::fs::read(payload.parent().unwrap().join("fonts.dat")).unwrap();
        let index: Index = serde_json::from_str(INDEX).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for (name, original) in [
            ("Inter", root.join("assets/inter_regular.ttf")),
            ("Inter Bold", root.join("assets/inter_semibold.ttf")),
        ] {
            let entry = index.fonts.iter().find(|f| f.name == name).unwrap();
            assert_eq!(
                &bytes[entry.offset..entry.offset + entry.bytes],
                std::fs::read(original).unwrap()
            );
        }
        assert_eq!(index.fonts.len(), 3);
        assert!(index
            .fonts
            .iter()
            .any(|font| font.name == "NotoEmoji-Regular"));
        let font_path = payload.parent().unwrap().join("fonts.dat");
        let mapping = Storage::open(&font_path).unwrap();
        assert!(File::options().write(true).open(&font_path).is_err());
        drop(mapping);
        let mut corrupted = bytes;
        corrupted[0] ^= 1;
        std::fs::write(&font_path, corrupted).unwrap();
        assert!(Storage::open(&font_path).is_err());
        std::fs::remove_dir_all(temporary).unwrap();
    }
}
