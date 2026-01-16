use anyhow::Context;
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::linux::fs::MetadataExt,
    path::PathBuf,
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

use fuser::{Filesystem, FUSE_ROOT_ID};

use crate::{cache::EntryCache, node::Node};

pub const TTL: std::time::Duration = std::time::Duration::from_secs(365 * 24 * 60 * 60);

pub struct ArchiveFs {
    inodes: HashMap<u64, Rc<Node>>,
    fhs: HashMap<u64, File>,
    next_fh: u64,
    entry_cache: EntryCache,
}

fn build_path_map(map: &mut HashMap<String, Rc<Node>>, nodes: &[Rc<Node>]) {
    for node in nodes {
        map.insert(node.path().to_string(), node.clone());
        if let Node::Directory { children, .. } = node.as_ref() {
            build_path_map(map, children);
        }
    }
}

fn convert_links(nodes: &[Rc<Node>], path_map: &HashMap<String, Rc<Node>>) -> Vec<Rc<Node>> {
    let mut converted_nodes = Vec::new();
    for node in nodes {
        match node.as_ref() {
            Node::Link { target, .. } => {
                let Some(target) = path_map.get(target) else {
                    log::warn!("Skipping link to unkown target: {target}");
                    continue;
                };
                converted_nodes.push(target.clone());
            }
            Node::Directory {
                index,
                name,
                path,
                mode,
                mtime,
                uid,
                gid,
                children,
            } => {
                converted_nodes.push(Rc::new(Node::Directory {
                    index: *index,
                    name: name.clone(),
                    path: path.clone(),
                    mode: *mode,
                    mtime: *mtime,
                    uid: *uid,
                    gid: *gid,
                    children: convert_links(children, path_map),
                }));
            }
            _ => converted_nodes.push(node.clone()),
        }
    }
    converted_nodes
}

fn build_inode_map(map: &mut HashMap<u64, Rc<Node>>, nodes: &[Rc<Node>]) {
    for node in nodes {
        match node.as_ref() {
            Node::File { index, .. } | Node::Symlink { index, .. } => {
                map.insert(*index, node.clone());
            }
            Node::Directory {
                index, children, ..
            } => {
                map.insert(*index, node.clone());
                build_inode_map(map, children);
            }
            _ => log::warn!("Skipping unexpected node: {node}"),
        }
    }
}

impl ArchiveFs {
    pub fn new(archive_path: String, root: Vec<Rc<Node>>) -> Self {
        // Replace links with their targets
        let mut path_map = HashMap::new();
        build_path_map(&mut path_map, &root);
        let root = convert_links(&root, &path_map);

        // Add dummy root node
        let archive_meta = || -> std::io::Result<(SystemTime, u32, u32)> {
            let meta = PathBuf::from(&archive_path).metadata()?;
            Ok((meta.modified()?, meta.st_uid(), meta.st_gid()))
        };
        let (mtime, uid, gid) = archive_meta().unwrap_or((UNIX_EPOCH, 0, 0));
        let mut dummy_root_node_children = Vec::with_capacity(2 + root.len());
        dummy_root_node_children.push(Rc::new(Node::Directory {
            index: FUSE_ROOT_ID,
            path: ".".to_string(),
            name: ".".to_string(),
            mode: 0o555,
            mtime,
            uid: uid as u64,
            gid: gid as u64,
            children: Vec::new(),
        }));
        dummy_root_node_children.push(Rc::new(Node::Directory {
            index: 0,
            path: "..".to_string(),
            name: "..".to_string(),
            mode: 0o555,
            mtime,
            uid: uid as u64,
            gid: gid as u64,
            children: Vec::new(),
        }));
        dummy_root_node_children.extend_from_slice(&root);
        let dummy_root_node = Node::Directory {
            index: FUSE_ROOT_ID,
            path: "".to_string(),
            name: "root".to_string(),
            mode: 0o555,
            mtime,
            uid: uid as u64,
            gid: gid as u64,
            children: dummy_root_node_children,
        };

        // Build inode map for fast lookups
        let mut inodes = HashMap::new();
        inodes.insert(dummy_root_node.index(), Rc::new(dummy_root_node));
        build_inode_map(&mut inodes, &root);

        Self {
            entry_cache: EntryCache::new(PathBuf::from(&archive_path), "/var/tmp/tarfs"),
            inodes,
            fhs: HashMap::new(),
            next_fh: 1,
        }
    }

    fn search(&mut self, inode: u64) -> Option<Rc<Node>> {
        self.inodes.get(&inode).cloned()
    }
}

impl Filesystem for ArchiveFs {
    fn destroy(&mut self) {
        if let Err(error) = self
            .entry_cache
            .clean()
            .context("Failed to clean entry cache")
        {
            log::error!("{error:?}");
        }
    }

    fn lookup(
        &mut self,
        _req: &fuser::Request<'_>,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        let Some(node) = self.search(parent) else {
            reply.error(libc::ENOENT);
            return;
        };

        match node.as_ref() {
            Node::Directory { children, .. } => {
                for child in children {
                    if child.name() == name {
                        reply.entry(&std::time::Duration::new(0, 0), &child.attr(), 0);
                        return;
                    }
                }
                reply.error(libc::ENOENT);
            }
            _ => reply.error(libc::ENOTDIR),
        }
    }

    fn getattr(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: Option<u64>,
        reply: fuser::ReplyAttr,
    ) {
        let Some(node) = self.search(ino) else {
            reply.error(libc::ENOENT);
            return;
        };

        reply.attr(&std::time::Duration::new(0, 0), &node.attr());
    }

    fn readlink(&mut self, _req: &fuser::Request<'_>, ino: u64, reply: fuser::ReplyData) {
        let node = match self.search(ino) {
            Some(inode) => inode,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        match node.as_ref() {
            Node::Symlink { target, .. } => {
                reply.data(target.as_bytes());
            }
            _ => {
                reply.error(libc::EINVAL);
            }
        }
    }

    fn open(&mut self, _req: &fuser::Request<'_>, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        let node = match self.search(ino) {
            Some(inode) => inode,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if matches!(node.as_ref(), Node::Directory { .. }) {
            reply.error(libc::EISDIR);
            return;
        }

        let fh = self.next_fh;
        self.next_fh += 1;

        let file = match self
            .entry_cache
            .open(node.path())
            .context("Failed to open cached file")
        {
            Ok(file) => file,
            Err(error) => {
                log::error!("{error:?}");
                reply.error(libc::EIO);
                return;
            }
        };

        self.fhs.insert(fh, file);
        reply.opened(fh, 0);
    }

    fn read(
        &mut self,
        _req: &fuser::Request<'_>,
        _ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        let Some(file) = self.fhs.get_mut(&fh) else {
            reply.error(libc::ENOENT);
            return;
        };

        let mut buf = vec![0; size as usize];
        let mut inner = || -> std::io::Result<usize> {
            file.seek(SeekFrom::Start(offset as u64))?;
            file.read(&mut buf)
        };
        match inner().context("Failed to read from cached file") {
            Ok(count) => reply.data(&buf[..count]),
            Err(error) => {
                log::error!("{error:?}");
                reply.error(libc::EIO);
            }
        }
    }

    fn release(
        &mut self,
        _req: &fuser::Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        self.fhs.remove(&fh);
        reply.ok();
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        let node = self.search(ino);
        let entries = match node.as_deref() {
            Some(Node::Directory { children, .. }) => children.iter(),
            Some(_) => {
                reply.error(libc::ENOTDIR);
                return;
            }
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        for (offset, entry) in entries.enumerate().skip(offset as usize) {
            let attr = entry.attr();
            if reply.add(attr.ino, (offset + 1) as i64, attr.kind, entry.name()) {
                break;
            }
        }
        reply.ok()
    }

    fn readdirplus(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectoryPlus,
    ) {
        let node = self.search(ino);
        let entries = match node.as_deref() {
            Some(Node::Directory { children, .. }) => children.iter(),
            Some(_) => {
                reply.error(libc::ENOTDIR);
                return;
            }
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        for (offset, entry) in entries.enumerate().skip(offset as usize) {
            let attr = entry.attr();
            if reply.add(attr.ino, (offset + 1) as i64, entry.name(), &TTL, &attr, 0) {
                break;
            }
        }
        reply.ok()
    }

    // TODO: Implement getxattr
    fn getxattr(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        name: &std::ffi::OsStr,
        size: u32,
        reply: fuser::ReplyXattr,
    ) {
        log::debug!(
            "[Not Implemented] getxattr(ino: {:#x?}, name: {:?}, size: {})",
            ino,
            name,
            size,
        );
        reply.error(libc::ENOSYS);
    }

    // TODO: Implement listxattr
    fn listxattr(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        size: u32,
        reply: fuser::ReplyXattr,
    ) {
        log::debug!(
            "[Not Implemented] listxattr(ino: {:#x?}, size: {})",
            ino,
            size,
        );
        reply.error(libc::ENOSYS);
    }
}
