# tarfs
Mount tarballs readonly via FUSE. Written in Rust.

## Usage
```
Mount a tar archive as a read-only file system

Usage: tarfs [OPTIONS] <ARCHIVE> <MOUNT_POINT>

Arguments:
  <ARCHIVE>      Path to the archive
  <MOUNT_POINT>  Mount point for the file system

Options:
      --auto-unmount  Unmount the file system automatically on exit
      --allow-root    Allow root to access the file system
      --allow-other   Allow other users to access the file system
      --dump-tree     Dump the file system tree to the debug log
  -h, --help          Print help
  -V, --version       Print version
```

Currently, `tarfs` handles uncompressed, bzip2, gzip, xz and zstd compressed tar archives.

## Build

Run `cargo build` to create a debug build at `target/debug/tarfs`.

Run `cargo build --release` to create a debug build at `target/release/tarfs`.

Run `cargo install --path=.` to install the binary to `~/.cargo/bin/tarfs`.
