use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use fs::ArchiveFs;
use fuser::MountOption;

use crate::{archive::open_archive, tree::TreeBuilder};

mod archive;
mod cache;
mod fs;
mod node;
mod tree;

/// Mount a tar archive as a read-only file system
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Unmount the file system automatically on exit
    #[clap(long)]
    auto_unmount: bool,

    /// Allow root to access the file system
    #[clap(long)]
    allow_root: bool,

    /// Allow other users to access the file system
    #[clap(long)]
    allow_other: bool,

    /// Dump the file system tree to the debug log
    #[clap(long)]
    dump_tree: bool,

    /// Path to the archive
    #[clap(required = true)]
    archive: String,

    /// Mount point for the file system
    #[clap(required = true)]
    mount_point: String,
}

fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let args = Args::parse();

    let archive_path = Utf8PathBuf::from(args.archive);
    let mount_point = Utf8PathBuf::from(args.mount_point);

    let mut archive = open_archive(&archive_path).context("Failed to open archive")?;
    let mut tree = TreeBuilder::new(archive.entries().context("Failed to read archive")?);
    let root = tree.build().context("Failed to build tree from archive")?;

    if args.dump_tree {
        let mut tree_buf = vec![b'\n'];
        for node in &root {
            node.print_tree(&mut tree_buf)?;
        }
        log::debug!("{}", String::from_utf8_lossy(&tree_buf));
    }

    let mut options = vec![MountOption::RO, MountOption::FSName("tarfs".to_string())];
    if args.auto_unmount {
        options.push(MountOption::AutoUnmount);
    }
    if args.allow_root {
        options.push(MountOption::AllowRoot);
    }
    if args.allow_other {
        options.push(MountOption::AllowOther);
    }

    let fs = ArchiveFs::new(archive_path.to_string(), root);
    fuser::mount2(fs, mount_point, &options).context("Failed to mount fuse file system")?;

    Ok(())
}
