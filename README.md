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

The log level can be configured via the `RUST_LOG` environment variable. Available log levels are
`trace`, `debug`, `info`, `warn` and `error`. The default log level is `info`.

## Build

Run `cargo build` to build a debug binary at `target/debug/tarfs`.

Run `cargo build --release` to build a release binary at `target/release/tarfs`.

Run `cargo install --path=.` to build and install a release binary to `~/.cargo/bin/tarfs`.
