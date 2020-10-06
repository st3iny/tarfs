package archive

import (
    "compress/bzip2"
    "compress/gzip"
    "fmt"
    "io"
    "os"

    "github.com/DataDog/zstd"
    "github.com/xi2/xz"
)

const (
    compressionNone string = "none"
    compressionBzip2 string = "bzip2"
    compressionGzip string = "gzip"
    compressionZstd string = "zstd"
    compressionXz string = "xz"
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

func isXz(file *os.File) bool {
    file.Seek(0, 0)
    _, err := xz.NewReader(file, 0)
    return err == nil
}

func getCompression(file *os.File) string {
    var compression string
    if isBzip2(file) {
        compression = compressionBzip2
    } else if isGzip(file) {
        compression = compressionGzip
    } else if isZstd(file) {
        compression = compressionZstd
    } else if isXz(file) {
        compression = compressionXz
    } else {
        compression = compressionNone
    }

    return compression
}

func decompress(file *os.File, compression string) (io.Reader, error) {
    file.Seek(0, 0)
    var reader io.Reader
    switch compression {
    case compressionBzip2:
        reader = bzip2.NewReader(file)
    case compressionGzip:
        reader, _ = gzip.NewReader(file)
    case compressionZstd:
        reader = zstd.NewReader(file)
    case compressionXz:
        var err error
        reader, err = xz.NewReader(file, 0)
        if err != nil {
            panic(err)
        }
    case compressionNone:
        reader = file
    default:
        return nil, fmt.Errorf(errorUnsupportedFormat)
    }

    return reader, nil
}
