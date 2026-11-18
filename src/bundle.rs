//! Content-addressed resources with streaming extraction and warm integrity checks.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};

pub const ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.zip"));
const INDEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/resource-index.zip"));
const ID: &str = include_str!(concat!(env!("OUT_DIR"), "/resource-id.txt"));
#[derive(Deserialize)]
struct FileSpec {
    bytes: u64,
    sha256: String,
}
type Index = BTreeMap<String, FileSpec>;
fn index() -> Result<(Vec<u8>, Index)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(INDEX))?;
    let mut bytes = Vec::new();
    archive.by_name("index.json")?.read_to_end(&mut bytes)?;
    let parsed = serde_json::from_slice(&bytes)?;
    Ok((bytes, parsed))
}
fn regular(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        bail!("Ссылки в app запрещены: {}", path.display());
    }
    use std::os::windows::fs::MetadataExt;
    if metadata.file_attributes() & 0x400 != 0 {
        bail!("Reparse point в app: {}", path.display());
    }
    Ok(())
}
fn safe_path(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name);
    if name.is_empty()
        || name.contains(['\\', ':'])
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("Небезопасный путь встроенного ресурса: {name}");
    }
    Ok(root.join(relative))
}
fn check_parents(path: &Path, root: &Path, checked: &mut HashSet<PathBuf>) -> Result<()> {
    let mut parent = path.parent();
    while let Some(p) = parent.filter(|p| p.starts_with(root)) {
        if checked.insert(p.to_path_buf()) {
            regular(p)?;
        }
        if p == root {
            break;
        }
        parent = p.parent();
    }
    Ok(())
}
fn transfer_checked(mut reader: impl Read, mut writer: impl Write, spec: &FileSpec) -> Result<()> {
    let mut remaining = spec.bytes;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 32768];
    while remaining > 0 {
        let limit = remaining.min(buffer.len() as u64) as usize;
        let read = reader.read(&mut buffer[..limit])?;
        if read == 0 {
            bail!("Усечённый ресурс");
        }
        hash.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
        remaining -= read as u64;
    }
    if format!("{:x}", hash.finalize()) != spec.sha256 {
        bail!("Контрольная сумма ресурса не совпадает");
    }
    Ok(())
}
fn verify(directory: &Path, index: &Index) -> Result<()> {
    regular(directory)?;
    let mut parents = HashSet::new();
    for (name, spec) in index {
        let path = safe_path(directory, name)?;
        check_parents(&path, directory, &mut parents)?;
        regular(&path)?;
        let file = fs::File::open(&path)?;
        if file.metadata()?.len() != spec.bytes {
            bail!("Повреждён размер app/{}", name);
        }
        transfer_checked(file, std::io::sink(), spec)
            .with_context(|| format!("Повреждён файл app/{name}"))?;
    }
    Ok(())
}
pub fn ensure(root: &Path) -> Result<PathBuf> {
    let app = root.join("app");
    fs::create_dir_all(&app).context(
        "Не удалось создать app рядом с EXE. Перенесите EXE в доступную для записи папку",
    )?;
    regular(&app)?;
    let (index_bytes, index) = index()?;
    let destination = app.join(ID);
    if destination.exists() {
        verify(&destination, &index).context(
            "Файлы app изменены. Сохраните повреждённый каталог отдельно и запустите EXE повторно",
        )?;
        return Ok(destination.join("payload"));
    }
    let staging = app.join(format!(".extract-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    let result = (|| -> Result<()> {
        let mut archive = zip::ZipArchive::new(Cursor::new(ARCHIVE))?;
        let mut stream = archive.by_name("resources.bin")?;
        let mut header = [0u8; 12];
        stream.read_exact(&mut header)?;
        if &header[..8] != b"HHRES001"
            || u32::from_le_bytes(header[8..].try_into()?) as usize != index_bytes.len()
        {
            bail!("Некорректный индекс ресурсов");
        }
        let mut actual_index = vec![0; index_bytes.len()];
        stream.read_exact(&mut actual_index)?;
        if actual_index != index_bytes {
            bail!("Индекс ресурсов изменён");
        }
        for (name, spec) in &index {
            let path = safe_path(&staging, name)?;
            fs::create_dir_all(path.parent().context("resource parent")?)?;
            // No path cache during extraction: every write rechecks its parents.
            check_parents(&path, &staging, &mut HashSet::new())?;
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            transfer_checked(&mut stream, file, spec)
                .with_context(|| format!("Не удалось распаковать {name}"))?;
            regular(&path)?;
        }
        if stream.read(&mut [0u8; 1])? != 0 {
            bail!("Лишние данные в ресурсах");
        }
        if let Err(error) = fs::rename(&staging, &destination) {
            if !destination.is_dir() {
                return Err(error.into());
            }
            verify(&destination, &index)?;
        }
        Ok(())
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(destination.join("payload"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extraction_reuse_integrity_and_data_preservation() {
        let root = std::env::temp_dir().join(format!("hh-bundle-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("data/keep"), b"untouched").unwrap();
        fs::create_dir_all(root.join("app/payload")).unwrap();
        fs::write(root.join("app/payload/old"), b"old").unwrap();
        let payload = ensure(&root).unwrap();
        assert!(payload.join("browser-agent/dist/main.js").is_file());
        assert!(!payload.join("browser-agent/node_modules").exists());
        assert!(payload
            .parent()
            .unwrap()
            .join("THIRD_PARTY_NOTICES.md")
            .is_file());
        assert_eq!(ensure(&root).unwrap(), payload);
        let original = fs::read(payload.join("browser-agent/dist/main.js")).unwrap();
        let mut changed = original.clone();
        changed[0] ^= 1;
        fs::write(payload.join("browser-agent/dist/main.js"), changed).unwrap();
        assert!(ensure(&root).is_err()); // Same-size corruption must not bypass integrity.
        fs::write(payload.join("browser-agent/dist/main.js"), original).unwrap();
        fs::write(payload.join("browser-agent/dist/main.js"), b"corrupt").unwrap();
        assert!(ensure(&root).is_err());
        assert_eq!(fs::read(root.join("data/keep")).unwrap(), b"untouched");
        assert_eq!(fs::read(root.join("app/payload/old")).unwrap(), b"old");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_escape_paths() {
        for name in ["../data/auth.json", "C:/file", "/file", "a\\b"] {
            assert!(safe_path(Path::new("root"), name).is_err());
        }
    }
}
