package main

import (
    "flag"
    "fmt"
    "log"
    "os"

    "github.com/st3iny/tarfs/internal/fs"
)

func main() {
    flag.Usage = usage
    flag.Parse()

    if flag.NArg() != 2 {
        usage()
        os.Exit(2)
    }
    archivePath := flag.Arg(0)
    mountpoint := flag.Arg(1)

    if err := fs.MountAndServe(archivePath, mountpoint); err != nil {
        log.Fatal(err)
    }
}


func usage() {
    fmt.Fprintf(os.Stderr, "Usage of %s:\n", os.Args[0])
    fmt.Fprintf(os.Stderr, "  %s ARCHIVE_PATH MOUNTPOINT\n", os.Args[0])
    flag.PrintDefaults()
}

