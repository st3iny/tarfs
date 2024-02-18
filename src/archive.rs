use std::{fs::File, io::Read, path::Path};

use anyhow::{bail, Context, Result};
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use tar::Archive;
use xz::read::XzDecoder;

pub fn open_archive(path: impl AsRef<Path>) -> Result<Archive<Box<dyn Read>>> {
    let mime_type = infer::get_from_path(&path)
        .context("Failed to infer archive type")?
        .context("File type of archive is unknown")?
        .mime_type();

    let archive = File::open(&path).context("Failed to open archive")?;
    let decompressor: Box<dyn Read> = match mime_type {
        "application/x-tar" => Box::new(archive),
        "application/gzip" => Box::new(GzDecoder::new(archive)),
        "application/x-xz" => Box::new(XzDecoder::new(archive)),
        "application/x-bzip2" => Box::new(BzDecoder::new(archive)),
        "application/zstd" => {
            Box::new(zstd::Decoder::new(archive).context("Failed to create zstd decoder")?)
        }
        _ => bail!("Unsupported archive or compression type: {mime_type}"),
    };

    Ok(tar::Archive::new(decompressor))
}
