package main

import (
    "flag"
    "fmt"
    "log"
    "os"

    "github.com/st3iny/tarfs/internal/fs"
)

func main() {
    var debug bool
    flag.BoolVar(&debug, "d", false, "Enable fuse debug mode")

    flag.Usage = usage
    flag.Parse()

    if flag.NArg() != 2 {
        usage()
        os.Exit(1)
    }
    archivePath := flag.Arg(0)
    mountpoint := flag.Arg(1)

    if err := fs.MountAndServe(archivePath, mountpoint, debug); err != nil {
        log.Fatal(err)
    }
}

func usage() {
    fmt.Fprintln(os.Stderr, "Usage: tarfs [-d] ARCHIVE_PATH MOUNTPOINT")
    flag.PrintDefaults()
}
