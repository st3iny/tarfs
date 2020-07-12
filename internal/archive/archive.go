package archive

import (
    "archive/tar"
    "fmt"
    "io"
    "log"
    "os"
    "path"
    "strings"
    "time"
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
}

type tarEntry struct {
    Index int
    Header *tar.Header
    Harvested bool
}

func ReadArchive(archivePath string) (*Archive, error) {
    file, err := os.Open(archivePath)
    if err != nil {
        return nil, err
    }

    var entries []*tarEntry
    arch := &Archive{Path: archivePath}
    reader := tar.NewReader(file)
    index := 0
    for {
        header, err := reader.Next()
        if err == io.EOF {
            break
        } else if err != nil {
            log.Panic(err)
        }

        entries = append(entries, &tarEntry{Index: index, Header: header, Harvested: false})
        index++
    }

    arch.Nodes = parseNodes(nil, entries, arch)
    return arch, nil
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

    reader := tar.NewReader(file)
    for i := 0; i <= node.index; i++ {
        if _, err := reader.Next(); err != nil {
            return nil, err
        }
    }

    return &NodeReader{file: file, reader: reader}, nil
}
