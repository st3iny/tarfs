use std::{
    fs::{create_dir_all, remove_dir_all, File},
    io::{Read, Seek, SeekFrom},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use tar::Archive;

use crate::archive::open_archive;

pub struct EntryCache {
    archive_path: PathBuf,
    base_dir: PathBuf,
}

impl EntryCache {
    pub fn new(archive_path: PathBuf, base_dir: impl AsRef<Path>) -> Self {
        let base_dir = base_dir.as_ref().join(hash_path(&archive_path));
        Self {
            archive_path,
            base_dir,
        }
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<File> {
        if !self.base_dir.exists() {
            create_dir_all(&self.base_dir).with_context(|| {
                format!(
                    "Failed to create cache directory: {}",
                    self.base_dir.display(),
                )
            })?;
        }

        let path = path.as_ref();
        let cached_path = self.base_dir.join(hash_path(path));
        if cached_path.exists() {
            log::debug!("Cache hit: {}", cached_path.display());
            return File::open(&cached_path)
                .with_context(|| format!("Failed to open cached file: {}", cached_path.display()));
        }

        log::debug!("Cache miss: {}", cached_path.display());
        for entry in self
            .archive()?
            .entries()
            .context("Failed to list archive entries")?
        {
            let mut entry = entry.context("Failed to read archive entry")?;
            if entry.path()? == path {
                let mut file = File::options()
                    .create_new(true)
                    .write(true)
                    .read(true)
                    .open(&cached_path)
                    .with_context(|| {
                        format!("Failed to create cached file: {}", cached_path.display())
                    })?;
                std::io::copy(&mut entry, &mut file)?;
                file.seek(SeekFrom::Start(0))?;
                return Ok(file);
            }
        }

        bail!("Entry does not exist in archive: {}", path.display());
    }

    pub fn clean(&self) -> std::io::Result<()> {
        remove_dir_all(&self.base_dir)
    }

    fn archive(&self) -> Result<Archive<Box<dyn Read>>> {
        open_archive(&self.archive_path).context("Failed to open archive")
    }
}

fn hash_path(path: impl AsRef<Path>) -> String {
    let mut hash = [0; 32];
    blake::hash(256, path.as_ref().as_os_str().as_bytes(), &mut hash).unwrap();
    hex::encode(hash)
}
