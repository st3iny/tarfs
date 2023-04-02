use anyhow::{bail, Context, Result};
use bzip2::read::BzDecoder;
use clap::Parser;
use flate2::read::GzDecoder;
use fork::{fork, Fork};
use fuser::{mount2, MountOption};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tar::Archive;
use xz2::read::XzDecoder;

use magic::Compression;

use crate::{filesystem::Filesystem, tree::Tree};

mod filesystem;
mod magic;
mod tree;

/// Mount a (uncompressed, gzip, bzip2, xz or zstd) tar archive as a read-only FUSE filesystem.
#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// Don't fork to background
    #[clap(short, long)]
    foreground: bool,

    /// Allow root to access the mount (requires user_allow_other in fuse.conf)
    #[clap(long)]
    allow_root: bool,

    /// Allow other users to access the mount (requires user_allow_other in fuse.conf)
    #[clap(long)]
    allow_other: bool,

    /// Use numeric user and group ids instead of resolving names
    #[clap(long)]
    numeric_owner: bool,

    /// Use a custom uid for all files
    #[clap(long)]
    uid: Option<u32>,

    /// Use a custom gid for all files
    #[clap(long)]
    gid: Option<u32>,

    /// Use a custom octal mode for all files and add search permissions to readable directories
    #[clap(long)]
    mode: Option<String>,

    /// Path to a *.tar.* archive
    #[clap(required = true)]
    archive: String,

    /// Path to mount the archive at
    #[clap(required = true)]
    mount_point: String,
}

impl Args {
    fn mount_opts(&self) -> Vec<MountOption> {
        let mut mount_opts = vec![
            MountOption::FSName(env!("CARGO_PKG_NAME").to_owned()),
            MountOption::RO,
            MountOption::NoDev,
        ];

        if self.allow_other || self.allow_root {
            mount_opts.push(MountOption::AllowOther);
        } else if self.allow_root {
            mount_opts.push(MountOption::AllowRoot);
        }

        mount_opts
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mode = match &args.mode {
        Some(mode) => Some(u16::from_str_radix(mode, 8).context("Failed to parse mode")?),
        None => None,
    };

    let archive_path = PathBuf::from(&args.archive);
    let file = File::open(&archive_path).context("Archive does not exist")?;
    let archive_mtime = file
        .metadata()
        .context("Failed to stat archive")?
        .modified()
        .context("Failed to get mtime of archive")?;

    let compression = Compression::detect(file).unwrap();

    let open_archive = move || {
        File::open(&archive_path)
            .and_then(|file| decompressor(compression, file))
            .map(Archive::new)
    };

    let archive = open_archive().context("Failed to open archive")?;

    println!("Building tree ... (this may take a while)");
    let tree = Tree::new(
        archive,
        archive_mtime,
        args.numeric_owner,
        args.uid,
        args.gid,
        mode,
    )
    .context("Failed to build file tree from archive")?;
    let filesystem = Filesystem::new(tree, open_archive);

    if args.foreground {
        println!("Mounting ...");
        do_mount(filesystem, &args.mount_point, &args.mount_opts())?;
    } else {
        println!("Forking and mounting ...");
        match fork() {
            Ok(Fork::Parent(child)) => {
                println!("Forked to background PID {child}")
            }
            Ok(Fork::Child) => {
                unsafe {
                    libc::close(libc::STDIN_FILENO);
                    libc::close(libc::STDOUT_FILENO);
                    libc::close(libc::STDERR_FILENO);
                };
                do_mount(filesystem, &args.mount_point, &args.mount_opts())?;
            }
            Err(error) => bail!("Failed to fork with code {error}"),
        }
    }

    Ok(())
}

fn do_mount(
    filesystem: impl fuser::Filesystem,
    mount_point: impl AsRef<Path>,
    opts: &[MountOption],
) -> Result<()> {
    mount2(filesystem, mount_point, opts).context("Failed to mount FUSE filesystem")
}

fn decompressor<'a>(
    compression: Compression,
    inner: impl Read + 'a,
) -> std::io::Result<Box<dyn Read + 'a>> {
    Ok(match compression {
        Compression::Gzip => Box::new(GzDecoder::new(inner)),
        Compression::Bzip2 => Box::new(BzDecoder::new(inner)),
        Compression::Xz => Box::new(XzDecoder::new(inner)),
        Compression::Zstd => Box::new(zstd::Decoder::new(inner)?),
        Compression::Unknown => {
            println!("Failed to detect compression -> assuming uncompressed archive");
            Box::new(inner)
        }
    })
}
