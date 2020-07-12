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
    "time"

    "github.com/DataDog/zstd"
)

const (
    errorUnsupportedFormat string = "Unsupported archive format"
    compressionNone string = "none"
    compressionBzip2 string = "bzip2"
    compressionGzip string = "gzip"
    compressionZstd string = "zstd"
)

type Node struct {
    index int
    Name string
    FullName string
    LinkName string
    Size int64
    Mode os.FileMode
    typeflag byte
    Uid int
    Gid int
    Mtime time.Time
    Atime time.Time
    Ctime time.Time
    Parent *Node
    Children []Node
    Archive *Archive
}

type Archive struct {
    Path string
    Nodes []Node
    compression string
}

type tarEntry struct {
    Index int
    Header *tar.Header
    Harvested bool
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

func (node *Node) listRecursive(nodes *[]*Node) {
    *nodes = append(*nodes, node)
    for _, child := range node.Children {
        child.listRecursive(nodes)
    }
}

func (node *Node) IsLink() bool {
    return node.typeflag == tar.TypeLink
}

func (node *Node) IsSymlink() bool {
    return node.typeflag == tar.TypeSymlink
}

func (arch *Archive) List() []*Node {
    var nodes []*Node
    for _, node := range arch.Nodes {
        node.listRecursive(&nodes)
    }
    return nodes
}

type NodeReader struct {
    file *os.File
    reader *tar.Reader
}

func (nodeReader *NodeReader) Read(buf []byte) (int, error) {
    return nodeReader.reader.Read(buf)
}

func (nodeReader *NodeReader) Close() error {
    return nodeReader.file.Close()
}

func (node *Node) Open() (io.ReadCloser, error) {
    if !node.Mode.IsRegular() {
        return nil, fmt.Errorf("Not a file")
    }

    file, err := os.Open(node.Archive.Path)
    if err != nil {
        return nil, err
    }

    reader, err := node.Archive.Read(file)
    if err != nil {
        return nil, err
    }

    for i := 0; i <= node.index; i++ {
        if _, err := reader.Next(); err != nil {
            return nil, err
        }
    }

    return &NodeReader{file: file, reader: reader}, nil
}
