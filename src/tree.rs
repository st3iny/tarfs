use std::{io::Read, rc::Rc};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use tar::{Entries, Entry};

use crate::node::Node;

pub struct TreeBuilder<'a, R: Read> {
    entries: Entries<'a, R>,
    head: Option<Entry<'a, R>>,
    next_index: u64,
}

impl<'a, R: Read> TreeBuilder<'a, R> {
    pub fn new(entries: Entries<'a, R>) -> Self {
        Self {
            entries,
            head: None,
            next_index: 1, // Skip fuse root ino (== 1)
        }
    }

    pub fn build(&mut self) -> Result<Vec<Rc<Node>>> {
        self.build_recursive(0)
    }

    fn build_recursive(&mut self, level: usize) -> Result<Vec<Rc<Node>>> {
        let mut nodes = Vec::new();
        loop {
            let entry = match self.head.take() {
                Some(entry) => entry,
                None => match self.entries.next() {
                    Some(entry) => entry.context("Failed to read archive entry")?,
                    None => break,
                },
            };

            let path = entry
                .path()
                .context("Failed to get path of entry")?
                .to_str()
                .context("Entry path is not utf8")?
                .to_string();
            let path = Utf8PathBuf::from(canonicalize_entry_path(path));

            self.next_index += 1;
            let Some(mut node) = Node::try_from_entry(&entry, self.next_index)
                .context("Failed to get node for archive entry")?
            else {
                log::warn!(
                    "Skipping unsupported entry type \"{:?}\" at {path}",
                    entry.header().entry_type(),
                );
                continue;
            };

            let entry_level = path
                .parent()
                .expect("Entry path has no parent")
                .components()
                .count();
            if entry_level < level {
                self.head = Some(entry);
                break;
            }

            if let Node::Directory { children, .. } = &mut node {
                children.extend(self.build_recursive(entry_level + 1)?);
            }

            nodes.push(Rc::new(node));
        }

        Ok(nodes)
    }
}

pub fn canonicalize_entry_path(path: impl AsRef<str>) -> String {
    path.as_ref()
        .trim_start_matches('.')
        .trim_start_matches('/')
        .to_string()
}
