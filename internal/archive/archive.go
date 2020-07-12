package archive

import (
    "archive/tar"
    "compress/bzip2"
    "compress/gzip"
    "fmt"
    "io"
    "os"
    "path"
    "strings"

    "github.com/DataDog/zstd"
)

const errorUnsupportedFormat string = "Unsupported archive format"

type tarEntry struct {
    Index int
    Header *tar.Header
    Harvested bool
}

type Archive struct {
    Path string
    Nodes []Node
    compression string
}

func ReadArchive(path string) (*Archive, error) {
    file, err := os.Open(path)
    if err != nil {
        return nil, err
    }
    defer file.Close()

    arch := &Archive{Path: path}

    if isBzip2(file) {
        arch.compression = compressionBzip2
    } else if isGzip(file) {
        arch.compression = compressionGzip
    } else if isZstd(file) {
        arch.compression = compressionZstd
    } else {
        arch.compression = compressionNone
    }

    var entries []*tarEntry
    if err != nil {
        return nil, err
    }

    tarReader, err := arch.Read(file)
    index := 0
    for {
        header, err := tarReader.Next()
        if err == io.EOF {
            break
        } else if err == tar.ErrHeader {
            return nil, fmt.Errorf(errorUnsupportedFormat)
        } else if err != nil {
            return nil, err
        }

        entries = append(entries, &tarEntry{Index: index, Header: header, Harvested: false})
        index++
    }

    arch.Nodes = parseNodes(nil, entries, arch)
    return arch, nil
}

func (arch *Archive) Read(file *os.File) (*tar.Reader, error) {
    file.Seek(0, 0)
    var reader io.Reader
    switch arch.compression {
    case compressionBzip2:
        reader = bzip2.NewReader(file)
    case compressionGzip:
        reader, _ = gzip.NewReader(file)
    case compressionZstd:
        reader = zstd.NewReader(file)
    case compressionNone:
        reader = file
    default:
        return nil, fmt.Errorf(errorUnsupportedFormat)
    }

    return tar.NewReader(reader), nil
}

func (arch *Archive) List() []*Node {
    var nodes []*Node
    for _, node := range arch.Nodes {
        node.listRecursive(&nodes)
    }
    return nodes
}

func parseNodes(parent *Node, entries []*tarEntry, arch *Archive) []Node {
    var nodes []Node
    parentReached := false
    for index, entry := range entries {
        file := entry.Header.Name
        isDir := entry.Header.FileInfo().IsDir()

        file = strings.TrimPrefix(file, "/")
        file = strings.TrimPrefix(file, "./")
        if file == "" {
            continue
        }

        // fast forward to current parent
        if parent != nil && !parentReached {
            if file == parent.FullName {
                parentReached = true
            }
            continue
        }

        // exit if all childs of parent have been recursively harvested
        if parent != nil && parentReached && !strings.HasPrefix(file, parent.FullName) {
            break
        }

        if parent == nil && !isDir && strings.Count(file, "/") > 0 {
            continue
        }

        if entry.Harvested {
            continue
        }

        entry.Harvested = true
        node := Node{
            index: entry.Index,
            Name: path.Base(file),
            FullName: file,
            LinkName: entry.Header.Linkname,
            Size: entry.Header.Size,
            Uid: entry.Header.Uid,
            Gid: entry.Header.Gid,
            Mode: entry.Header.FileInfo().Mode(),
            typeflag: entry.Header.Typeflag,
            Mtime: entry.Header.ModTime,
            Atime: entry.Header.AccessTime,
            Ctime: entry.Header.ChangeTime,
            Archive: arch,
            Parent: parent,
        }

        if isDir {
            node.Children = parseNodes(&node, entries[index:], arch)
        }

        nodes = append(nodes, node)
    }

    return nodes
}
