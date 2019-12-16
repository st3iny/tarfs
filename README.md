# tarfs

Mount tarballs readonly via FUSE. Written in go.

## Build

Requires a working go installation. The makefile will overwrite your `$GOPATH` by setting it to the
project directory. All the requirements and binaries will be placed locally in this project 
directory to prevent cluttering.

Fetch all dependencies via `make deps` and build the binary via `make build`.

## WIP

This project still is under heavy development and is highly unstable at the moment!
