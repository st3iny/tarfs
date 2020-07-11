# tarfs
Mount tarballs readonly via FUSE. Written in go.

## Usage
```
Usage: tarfs [-d] ARCHIVE_PATH MOUNTPOINT
  -d	Enable fuse debug mode
```

## Build
Run `make` to build the binary `tarfs`.

Run `make install` to install the binary to `/usr/local/bin`.
A custom install directory can be set via the `INSTALL_DIR` environment variable.
