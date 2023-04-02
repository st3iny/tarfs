use fuser::consts::FOPEN_KEEP_CACHE;
use std::{cmp::min, collections::HashMap, io::Read, os::unix::prelude::OsStrExt, time::Duration};
use tar::Archive;

use libc::ENOENT;

use crate::tree::Tree;

const TTL: Duration = Duration::from_secs(365 * 24 * 3600);

struct FileHandle {
    buf: Option<Vec<u8>>,
}

pub struct Filesystem<R: Read> {
    tree: Tree,
    open_archive: Box<dyn Fn() -> std::io::Result<Archive<R>>>,
    next_fh: u64,
    handles: HashMap<u64, FileHandle>,
}

impl<R: Read> Filesystem<R> {
    pub fn new(
        tree: Tree,
        open_archive: impl Fn() -> std::io::Result<Archive<R>> + 'static,
    ) -> Self {
        Self {
            tree,
            open_archive: Box::new(open_archive),
            next_fh: 0,
            handles: HashMap::new(),
        }
    }

    fn handle(&mut self) -> u64 {
        let fh = self.next_fh;
        self.next_fh += 1;
        fh
    }
}

impl<R: Read> fuser::Filesystem for Filesystem<R> {
    fn getattr(&mut self, _req: &fuser::Request<'_>, ino: u64, reply: fuser::ReplyAttr) {
        match self.tree.find(ino) {
            Some(node) => reply.attr(&TTL, &node.attr()),
            None => reply.error(ENOENT),
        }
    }

    fn lookup(
        &mut self,
        _req: &fuser::Request<'_>,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        match self.tree.lookup(parent, name) {
            Some(node) => reply.entry(&TTL, &node.attr(), node.id()),
            None => reply.error(ENOENT),
        }
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        let node = match self.tree.find(ino) {
            Some(node) => node,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let entries = match node.children() {
            Some(entries) => entries,
            None => {
                // FIXME: correct error type
                reply.error(ENOENT);
                return;
            }
        };

        for (entry, i) in entries.iter().zip(1i64..).skip(offset as usize) {
            if reply.add(entry.id(), i, entry.attr().kind, entry.name()) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&mut self, _req: &fuser::Request<'_>, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        if self.tree.find(ino).is_none() {
            reply.error(ENOENT);
            return;
        };

        let fh = self.handle();
        self.handles.insert(fh, FileHandle { buf: None });
        reply.opened(fh, FOPEN_KEEP_CACHE);
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
        self.handles.remove(&fh);
        reply.ok();
    }

    fn read(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        println!("read({fh})");

        let handle = match self.handles.get_mut(&fh) {
            Some(handle) => handle,
            None => {
                // FIXME: correct error type
                reply.error(ENOENT);
                return;
            }
        };

        let buf = match &handle.buf {
            Some(buf) => buf,
            None => {
                let node = match self.tree.find(ino) {
                    Some(node) => node,
                    None => {
                        reply.error(ENOENT);
                        return;
                    }
                };

                let archive = match (self.open_archive)() {
                    Ok(archive) => archive,
                    Err(error) => {
                        eprintln!("Error opening archive: {error:?}");
                        // FIXME: correct error type
                        reply.error(ENOENT);
                        return;
                    }
                };

                // FIXME: don't buffer very large files
                let buf = match node.read(archive, 0, None) {
                    Ok(buf) => buf,
                    Err(error) => {
                        eprintln!("Error reading entry: {error:?}");
                        // FIXME: correct error type
                        reply.error(ENOENT);
                        return;
                    }
                };

                handle.buf.insert(buf)
            }
        };

        let start = offset as usize;
        let end = min(start + size as usize, buf.len());
        let data = &buf[start..end];
        reply.data(data);
    }

    fn readlink(&mut self, _req: &fuser::Request<'_>, ino: u64, reply: fuser::ReplyData) {
        let node = match self.tree.find(ino) {
            Some(node) => node,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        match node.link_target() {
            Some(target) => reply.data(target.as_bytes()),
            None => {
                // FIXME: correct error type
                reply.error(ENOENT);
            }
        }
    }
}
