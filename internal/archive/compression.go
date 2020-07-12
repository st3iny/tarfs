package archive

import (
    "compress/bzip2"
    "compress/gzip"
    "os"

    "github.com/DataDog/zstd"
)

const (
    compressionNone string = "none"
    compressionBzip2 string = "bzip2"
    compressionGzip string = "gzip"
    compressionZstd string = "zstd"
)

func isBzip2(file *os.File) bool {
    file.Seek(0, 0)
    reader := bzip2.NewReader(file)
    buf := make([]byte, 16)
    _, err := reader.Read(buf)
    return err == nil
}

func isGzip(file *os.File) bool {
    file.Seek(0, 0)
    _, err := gzip.NewReader(file)
    return err == nil
}

func isZstd(file *os.File) bool {
    file.Seek(0, 0)
    reader := zstd.NewReader(file)
    buf := make([]byte, 16)
    _, err := reader.Read(buf)
    return err == nil
}
