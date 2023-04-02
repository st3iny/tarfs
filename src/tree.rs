use std::{
    cell::{Ref, RefCell},
    cmp::min,
    ffi::{OsStr, OsString},
    fmt::Write,
    io::{Read, Seek, SeekFrom},
    ops::Deref,
    os::unix::prelude::{OsStrExt, OsStringExt},
    path::{Components, Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use fuser::{FileAttr, FileType};
use tar::{Archive, EntryType};
use users::{get_group_by_name, get_user_by_name};

pub struct Tree {
    root: Rc<Node>,
}

impl Tree {
    pub fn new<R>(
        mut archive: Archive<R>,
        root_mtime: SystemTime,
        numeric_owner: bool,
        force_uid: Option<u32>,
        force_gid: Option<u32>,
        force_mode: Option<u16>,
    ) -> Result<Tree>
    where
        R: Read,
    {
        let resolve_uid = |name: Option<&[u8]>, uid: std::io::Result<u64>| -> Result<u32> {
            if let Some(uid) = force_uid {
                return Ok(uid);
            }

            if numeric_owner {
                return uid
                    .map(|uid| uid as u32)
                    .context("Failed to get uid of entry");
            }

            name.and_then(|name| get_user_by_name(OsStr::from_bytes(name)))
                .map(|user| user.uid())
                .or_else(|| uid.ok().map(|uid| uid as u32))
                .ok_or_else(|| anyhow!("Failed to get uid of entry"))
        };

        let resolve_gid = |name: Option<&[u8]>, gid: std::io::Result<u64>| -> Result<u32> {
            if let Some(gid) = force_gid {
                return Ok(gid);
            }

            if numeric_owner {
                return gid
                    .map(|gid| gid as u32)
                    .context("Failed to get gid of entry");
            }

            name.and_then(|name| get_group_by_name(OsStr::from_bytes(name)))
                .map(|group| group.gid())
                .or_else(|| gid.ok().map(|uid| uid as u32))
                .ok_or_else(|| anyhow!("Failed to get gid of entry"))
        };

        let calculate_perms = |header: &tar::Header| -> Result<u16> {
            if let Some(mut mode) = force_mode {
                mode &= 0o777;

                // Propagate read bits to execte bits for directories
                if header.entry_type() == EntryType::Directory {
                    mode |= (mode & 0o444) >> 2;
                }

                return Ok(mode);
            }

            let mode = header.mode()? & 0o777;
            Ok(mode as u16)
        };

        let root = Node::Directory {
            id: 1,
            name: OsString::new(),
            children: RefCell::new(Vec::new()),
            mtime: root_mtime,
            uid: 0,
            gid: 0,
            perms: 0o555,
        };

        let mut links: Vec<Link> = Vec::new();

        let mut next_id = root.id() + 1;
        let entries = archive
            .entries()
            .context("Failed to iterate archive entries")?;
        for (archive_index, entry) in entries.enumerate() {
            let entry = entry?;

            /*
            eprintln!(
                "Entry: {:?} {:?}",
                entry.header().entry_type(),
                entry.path().unwrap(),
            );
            */

            let path = entry.path()?;
            let name = match path.file_name() {
                Some(name) => name.to_owned(),
                None => {
                    eprintln!("Skipping entry with no file name: {:?}", path);
                    continue;
                }
            };
            let header = entry.header();
            let entry_type = header.entry_type();
            let node = match entry_type {
                tar::EntryType::Regular => Node::File {
                    id: next_id,
                    name,
                    size: entry.size(),
                    mtime: unix_to_system_time(header.mtime()?),
                    uid: resolve_uid(header.username_bytes(), header.uid())?,
                    gid: resolve_gid(header.groupname_bytes(), header.gid())?,
                    perms: calculate_perms(header)?,
                    archive_index: archive_index as u64,
                    link_count: AtomicU32::new(1),
                },
                tar::EntryType::Link => {
                    links.push(Link {
                        path: path.as_os_str().to_owned(),
                        name,
                        target: OsString::from_vec(
                            entry
                                .link_name_bytes()
                                .ok_or_else(|| anyhow!("Missing link name"))?
                                .to_vec(),
                        ),
                    });
                    continue;
                }
                tar::EntryType::Symlink => Node::Symlink {
                    id: next_id,
                    name,
                    target: OsString::from_vec(
                        entry
                            .link_name_bytes()
                            .ok_or_else(|| anyhow!("Missing link name"))?
                            .to_vec(),
                    ),
                    mtime: unix_to_system_time(header.mtime()?),
                    uid: resolve_uid(header.username_bytes(), header.uid())?,
                    gid: resolve_gid(header.groupname_bytes(), header.gid())?,
                    link_count: AtomicU32::new(1),
                },
                tar::EntryType::Directory => Node::Directory {
                    id: next_id,
                    name,
                    children: RefCell::new(Vec::new()),
                    mtime: unix_to_system_time(header.mtime()?),
                    uid: resolve_uid(header.username_bytes(), header.uid())?,
                    gid: resolve_gid(header.groupname_bytes(), header.gid())?,
                    perms: calculate_perms(header)?,
                },
                _ => {
                    eprintln!("Skipping entry with unknown type: {entry_type:?} {path:?}");
                    continue;
                }
            };

            next_id += 1;
            if let Some(_node) = push_child_node(&root, &path, node) {
                bail!("Skipping orphaned {:?}: {path:?}", header.entry_type());
            }
        }

        let tree = Tree {
            root: Rc::new(root),
        };

        for link in links {
            let target_path = PathBuf::from(&link.target);
            let target_node = match tree.walk(&target_path) {
                Some(node) => node,
                None => {
                    eprintln!("Skipping link: Target does not exist: {:?}", link.target);
                    continue;
                }
            };

            let link_path = PathBuf::from(&link.path);
            match push_child_node(&tree.root, &link_path, link.into_node(target_node.clone())) {
                Some(_node) => {
                    eprintln!("Skipping orphaned link: {link_path:?}");
                    continue;
                }
                None => {
                    if let Err(error) = target_node.inc_link_count() {
                        eprintln!("Failed to increment link count: {:?}", error);
                        continue;
                    }
                }
            }
        }

        Ok(tree)
    }

    pub fn find(&self, id: u64) -> Option<Rc<Node>> {
        self.root.find(id)
    }

    pub fn lookup(&self, parent: u64, name: &OsStr) -> Option<Rc<Node>> {
        self.root.lookup(parent, name)
    }

    pub fn walk(&self, path: impl AsRef<Path>) -> Option<Rc<Node>> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Some(self.root.clone());
        }

        let mut current_node = self.root.clone();
        for component in path.components() {
            let parent = current_node.clone();
            for node in parent.children()?.iter() {
                if node.name() == component.as_os_str() {
                    current_node = node.clone();
                    break;
                }
            }
        }

        Some(current_node)
    }
}

impl std::fmt::Display for Tree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.root.display(f, -1)
    }
}

struct Link {
    path: OsString,
    name: OsString,
    target: OsString,
}

impl Link {
    fn into_node(self, target: Rc<Node>) -> Node {
        Node::Link {
            name: self.name,
            target,
        }
    }
}

pub enum Node {
    Directory {
        id: u64,
        name: OsString,
        children: RefCell<Vec<Rc<Node>>>,
        mtime: SystemTime,
        uid: u32,
        gid: u32,
        perms: u16,
    },
    File {
        id: u64,
        name: OsString,
        size: u64,
        mtime: SystemTime,
        uid: u32,
        gid: u32,
        perms: u16,
        archive_index: u64,
        link_count: AtomicU32,
    },
    Symlink {
        id: u64,
        name: OsString,
        target: OsString,
        mtime: SystemTime,
        uid: u32,
        gid: u32,
        link_count: AtomicU32,
    },
    Link {
        name: OsString,
        target: Rc<Node>,
    },
}

impl Node {
    pub fn id(&self) -> u64 {
        match self {
            Node::File { id, .. } => *id,
            Node::Directory { id, .. } => *id,
            Node::Symlink { id, .. } => *id,
            Node::Link { target, .. } => target.id(),
        }
    }

    pub fn name(&self) -> &OsStr {
        match self {
            Node::File { name, .. } => name,
            Node::Directory { name, .. } => name,
            Node::Symlink { name, .. } => name,
            Node::Link { name, .. } => name,
        }
    }

    pub fn link_target(&self) -> Option<&OsStr> {
        match self {
            Node::Symlink { target, .. } => Some(target),
            _ => None,
        }
    }

    pub fn attr(&self) -> FileAttr {
        match self {
            Node::Directory {
                id,
                mtime,
                uid,
                gid,
                perms,
                ..
            } => FileAttr {
                ino: *id,
                size: 0,
                blocks: 0,
                atime: *mtime,
                mtime: *mtime,
                ctime: *mtime,
                crtime: *mtime,
                kind: FileType::Directory,
                perm: *perms,
                nlink: 1,
                uid: *uid,
                gid: *gid,
                rdev: 0,
                blksize: 0,
                flags: 0,
            },
            Node::File {
                id,
                size,
                mtime,
                uid,
                gid,
                perms,
                link_count,
                ..
            } => FileAttr {
                ino: *id,
                size: *size,
                blocks: 0,
                atime: *mtime,
                mtime: *mtime,
                ctime: *mtime,
                crtime: *mtime,
                kind: FileType::RegularFile,
                perm: *perms,
                nlink: link_count.load(Ordering::Relaxed),
                uid: *uid,
                gid: *gid,
                rdev: 0,
                blksize: 0,
                flags: 0,
            },
            Node::Symlink {
                id,
                target,
                mtime,
                uid,
                gid,
                link_count,
                ..
            } => FileAttr {
                ino: *id,
                size: target.len() as u64 + 1,
                blocks: 0,
                atime: *mtime,
                mtime: *mtime,
                ctime: *mtime,
                crtime: *mtime,
                kind: FileType::Symlink,
                perm: 0o777,
                nlink: link_count.load(Ordering::Relaxed),
                uid: *uid,
                gid: *gid,
                rdev: 0,
                blksize: 0,
                flags: 0,
            },
            Node::Link { target, .. } => target.attr(),
        }
    }

    pub fn children(&self) -> Option<Ref<Vec<Rc<Node>>>> {
        match self {
            Node::Directory { children, .. } => Some(children.borrow()),
            _ => None,
        }
    }

    pub fn read<R>(
        &self,
        mut archive: Archive<R>,
        offset: u64,
        length: Option<u64>,
    ) -> Result<Vec<u8>>
    where
        R: Read,
    {
        if !matches!(self, Node::File { .. }) {
            bail!("Not a file");
        }

        let archive_index = match self {
            Node::File { archive_index, .. } => *archive_index,
            _ => unreachable!(),
        };

        let mut entries = archive.entries()?.skip(archive_index as usize);
        let mut entry = entries.next().unwrap()?;

        let offset = min(offset, entry.size());
        let size = length
            .map(|length| min(length, entry.size() - offset))
            .unwrap_or(entry.size() - offset);
        dbg!(offset, length, size, entry.size());
        let mut buf = vec![0; size as usize];

        // Fake seek if offset is specified
        if offset > 0 {
            for _ in 0..(offset / size) {
                entry.read_exact(&mut buf)?;
            }
            let rest = (offset % size) as usize;
            if rest > 0 {
                entry.read_exact(&mut buf[..rest])?;
            }
        }

        // Read actual content
        entry.read_exact(&mut buf)?;
        assert_eq!(buf.len(), size as usize);

        Ok(buf)
    }

    /*
    pub fn open<R>(&self, mut archive: R) -> Result<FileNodeReader<R>>
    where
        R: Read + Seek,
    {
        match self {
            Node::File { size, offset, .. } => {
                archive.seek(SeekFrom::Start(*offset))?;
                Ok(FileNodeReader {
                    inner: archive,
                    offset: *offset,
                    length: *size,
                    pos: 0,
                })
            }
            _ => bail!("Not a file"),
        }
    }
    */

    fn push_child_unchecked(&self, child: Node) {
        match self {
            Node::Directory { children, .. } => children.borrow_mut().push(Rc::new(child)),
            _ => panic!("Cannot push children to a file"),
        }
    }

    fn inc_link_count(&self) -> Result<()> {
        match self {
            Node::File { link_count, .. } => link_count.fetch_add(1, Ordering::Relaxed),
            Node::Symlink { link_count, .. } => link_count.fetch_add(1, Ordering::Relaxed),
            _ => bail!("Can only increment link count of a files and symlinks"),
        };
        Ok(())
    }

    fn display(&self, f: &mut std::fmt::Formatter<'_>, level: i32) -> std::fmt::Result {
        for _ in 0..level {
            f.write_str("  ")?;
        }
        write!(f, "{}", self.name().to_string_lossy())?;
        if matches!(self, Node::Directory { .. }) {
            f.write_char('/')?;
        }
        f.write_char('\n')?;

        if let Node::Directory { children, .. } = self {
            for child in children.borrow().iter() {
                child.display(f, level + 1)?;
            }
        }

        Ok(())
    }

    fn find(self: &Rc<Node>, other_id: u64) -> Option<Rc<Node>> {
        if self.id() == other_id {
            return Some(self.clone());
        }

        if let Node::Directory { children, .. } = self.deref() {
            for child in children.borrow().iter() {
                if let Some(node) = child.find(other_id) {
                    return Some(node);
                }
            }
        }

        None
    }

    fn lookup(&self, parent: u64, name: &OsStr) -> Option<Rc<Node>> {
        match self {
            Node::Directory { id, children, .. } if *id == parent => {
                for child in children.borrow().iter() {
                    if child.name() == name {
                        return Some(child.clone());
                    }
                }
            }
            Node::Directory { children, .. } => {
                for child in children.borrow().iter() {
                    if let Some(node) = child.lookup(parent, name) {
                        return Some(node);
                    }
                }
            }
            _ => {}
        }

        None
    }
}

pub struct FileNodeReader<R: Read + Seek> {
    inner: R,
    offset: u64,
    length: u64,
    pos: u64,
}

impl<R: Read + Seek> Read for FileNodeReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = min((self.length - self.pos) as usize, buf.len());
        if n == 0 {
            return Ok(0);
        }

        self.inner.read(&mut buf[..n])
    }
}

impl<R: Read + Seek> Seek for FileNodeReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match pos {
            SeekFrom::Start(offset) => {
                let offset = min(offset, self.length);
                let actual = self.inner.seek(SeekFrom::Start(self.offset + offset))?;
                Ok(actual - self.offset)
            }
            SeekFrom::End(offset) => {
                if offset < 0 && self.pos.checked_sub(-offset as u64).is_none() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Trying to seek to negative offset",
                    ));
                }

                // Seek to the end because seeking past the end of a file is not meaningful in a
                // read-only file system
                let actual = self
                    .inner
                    .seek(SeekFrom::Start(self.offset + self.length))?;
                Ok(actual - self.offset)
            }
            SeekFrom::Current(offset) => {
                if offset < 0 && self.pos.checked_sub(-offset as u64).is_none() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Trying to seek to negative offset",
                    ));
                }

                if offset > 0 && self.pos + offset as u64 > self.length {
                    let actual = self
                        .inner
                        .seek(SeekFrom::Start(self.offset + self.length))?;
                    return Ok(actual - self.offset);
                }

                let actual = self.inner.seek(SeekFrom::Current(offset))?;
                Ok(actual - self.offset)
            }
        }
    }
}

fn push_child_node(root: &Node, path: impl AsRef<Path>, child: Node) -> Option<Node> {
    fn inner(node: &Node, mut components: Components, child: Node) -> Option<Node> {
        match node {
            Node::Directory { children, .. } => match components.next() {
                None => {
                    children.borrow_mut().push(Rc::new(child));
                    None
                }
                Some(next) => {
                    for children in children.borrow().iter() {
                        if children.name() == next.as_os_str() {
                            return inner(children, components, child);
                        }
                    }
                    Some(child)
                }
            },
            _ => Some(child),
        }
    }

    let parent = match path.as_ref().parent() {
        None => {
            root.push_child_unchecked(child);
            return None;
        }
        Some(parent) => parent,
    };
    inner(root, parent.components(), child)
}

fn unix_to_system_time(unix: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(unix)
}
