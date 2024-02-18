use std::{
    fmt::Display,
    io::{Read, Write},
    ops::Add,
    rc::Rc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use fuser::{FileAttr, FileType};
use tar::{Entry, EntryType};

#[derive(Debug)]
pub enum Node {
    File {
        index: u64,
        name: String,
        path: String,
        size: u64,
        mode: u32,
        mtime: SystemTime,
        uid: u64,
        gid: u64,
    },
    Directory {
        index: u64,
        name: String,
        path: String,
        mode: u32,
        mtime: SystemTime,
        uid: u64,
        gid: u64,
        children: Vec<Rc<Node>>,
    },
    Symlink {
        index: u64,
        name: String,
        path: String,
        mtime: SystemTime,
        uid: u64,
        gid: u64,
        target: String,
    },
    Link {
        index: u64,
        name: String,
        path: String,
        target: String,
    },
}

impl Node {
    pub fn try_from_entry<R: Read>(entry: &'_ Entry<'_, R>, index: u64) -> Result<Option<Self>> {
        let path_buf = Utf8PathBuf::try_from(
            entry
                .path()
                .context("Failed to get path of entry")?
                .to_path_buf(),
        )
        .context("Entry path is not utf8")?;
        let name = path_buf
            .file_name()
            .context("Failed to get file name of entry")?
            .to_string();
        let path = path_buf.to_string();
        let mode = entry.header().mode().context("Failed to get entry perms")?;
        let uid = entry.header().uid().context("Failed to get entry uid")?;
        let gid = entry.header().gid().context("Failed to get entry gid")?;
        let mtime = SystemTime::UNIX_EPOCH.add(Duration::from_secs(
            entry
                .header()
                .mtime()
                .context("Failed to get entry mtime")?,
        ));
        let link_target = || -> Result<String> {
            Ok(entry
                .link_name()
                .context("Failed to get link target")?
                .expect("Link has no target")
                .to_str()
                .context("Link target is not utf8")?
                .to_string())
        };
        let node = match entry.header().entry_type() {
            EntryType::Directory => Node::Directory {
                index,
                name,
                path,
                mode,
                mtime,
                uid,
                gid,
                children: Vec::new(),
            },
            EntryType::Regular => Node::File {
                index,
                name,
                path,
                size: entry.header().size()?,
                mode,
                mtime,
                uid,
                gid,
            },
            EntryType::Symlink => Node::Symlink {
                index,
                name,
                path,
                mtime,
                uid,
                gid,
                target: link_target()?,
            },
            EntryType::Link => Node::Link {
                index,
                name,
                path,
                target: link_target()?,
            },
            _ => return Ok(None),
        };
        Ok(Some(node))
    }

    pub fn index(&self) -> u64 {
        match self {
            Node::File { index, .. } => *index,
            Node::Directory { index, .. } => *index,
            Node::Symlink { index, .. } => *index,
            Node::Link { index, .. } => *index,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Node::File { name, .. } => name,
            Node::Directory { name, .. } => name,
            Node::Symlink { name, .. } => name,
            Node::Link { name, .. } => name,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Node::File { path, .. } => path,
            Node::Directory { path, .. } => path,
            Node::Symlink { path, .. } => path,
            Node::Link { path, .. } => path,
        }
    }

    pub fn attr(&self) -> FileAttr {
        match self {
            Node::File {
                index,
                size,
                mode,
                mtime,
                uid,
                gid,
                ..
            } => FileAttr {
                ino: *index,
                size: *size,
                blocks: 0,
                atime: *mtime,
                mtime: *mtime,
                ctime: *mtime,
                crtime: *mtime,
                kind: FileType::RegularFile,
                perm: *mode as u16,
                nlink: 1,
                uid: *uid as u32,
                gid: *gid as u32,
                rdev: 0,
                blksize: 0,
                flags: 0,
            },
            Node::Directory {
                index,
                mode,
                mtime,
                uid,
                gid,
                ..
            } => FileAttr {
                ino: *index,
                size: 0,
                blocks: 0,
                atime: *mtime,
                mtime: *mtime,
                ctime: *mtime,
                crtime: *mtime,
                kind: FileType::Directory,
                perm: *mode as u16,
                nlink: 1,
                uid: *uid as u32,
                gid: *gid as u32,
                rdev: 0,
                blksize: 0,
                flags: 0,
            },
            Node::Symlink {
                index,
                mtime,
                uid,
                gid,
                target,
                ..
            } => FileAttr {
                ino: *index,
                size: target.len() as u64,
                blocks: 0,
                atime: *mtime,
                mtime: *mtime,
                ctime: *mtime,
                crtime: *mtime,
                kind: FileType::Symlink,
                perm: 0o777,
                nlink: 1,
                uid: *uid as u32,
                gid: *gid as u32,
                rdev: 0,
                blksize: 0,
                flags: 0,
            },
            Node::Link { .. } => panic!("Can't get file attributes of a link"),
        }
    }

    pub fn print_tree(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.print_tree_recursive(writer, 0)
    }

    fn print_tree_recursive(&self, writer: &mut impl Write, indent: usize) -> std::io::Result<()> {
        for _ in 0..indent {
            write!(writer, "  ")?;
        }
        match self {
            Node::File { name, .. } => {
                writeln!(writer, "{name}")?;
            }
            Node::Directory { name, children, .. } => {
                writeln!(writer, "{name}")?;
                for child in children {
                    child.print_tree_recursive(writer, indent + 1)?;
                }
            }
            Node::Symlink { name, target, .. } => {
                writeln!(writer, "{name} -> {target}")?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Node::File { .. } => "File",
            Node::Directory { .. } => "Directory",
            Node::Symlink { .. } => "Symlink",
            Node::Link { .. } => "Link",
        };
        write!(f, "{kind}")
    }
}
