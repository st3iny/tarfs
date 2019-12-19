package main

import (
    "archive/tar"
    "io"
    "log"
    "os"
    "path"
    "strings"
    "time"
)

type Node struct {
    index uint
    typeflag byte
    Name string
    FullName string
    LinkName string
    Size int64
    Mode os.FileMode
    Uid int
    Gid int
    Mtime time.Time
    Atime time.Time
    Ctime time.Time
    FileInfo os.FileInfo
    Parent *Node
    Children []Node
    Archive *Archive
}

type Archive struct {
    Path string
    Nodes []Node
}

type Header struct {
    Header *tar.Header
    Harvested bool
}

func ReadArchive(archivePath string) (*Archive, error) {
    file, err := os.Open(archivePath)
    if err != nil {
        return nil, err
    }

    archive := &Archive{Path: archivePath}

    headers := []*Header{}
    reader := tar.NewReader(file)
    for {
        header, err := reader.Next();
        if err == io.EOF {
            break
        }

        headers = append(headers, &Header{Header: header, Harvested: false})
    }

    archive.Nodes = parseNodes("", headers, archive)

    for _, header := range headers {
        if !header.Harvested {
            log.Println("orphaned node at", header.Header.Name)
        }
    }

    return archive, nil
}

func parseNodes(name string, entries []*Header, archive *Archive) []Node {
    nodes := []Node{}
    for index, entry := range entries {
        file := entry.Header.Name
        if entry.Harvested || file == name || !strings.HasPrefix(file, name) {
            continue
        }

        fileIsDirectory := strings.HasSuffix(file, "/")
        if name != "" {
            fileSlashCount := strings.Count(file, "/")
            nodeSlashCount := strings.Count(name, "/")
            if fileIsDirectory && fileSlashCount > nodeSlashCount + 1 {
                continue
            } else if !fileIsDirectory && fileSlashCount > nodeSlashCount {
                continue
            }
        }

        entry.Harvested = true
        node := Node{
            index: uint(index),
            typeflag: entry.Header.Typeflag,
            Name: path.Base(file),
            FullName: file,
            LinkName: entry.Header.Linkname,
            Size: entry.Header.Size,
            Uid: entry.Header.Uid,
            Gid: entry.Header.Gid,
            Mode: entry.Header.FileInfo().Mode(),
            FileInfo: entry.Header.FileInfo(),
            Mtime: entry.Header.ModTime,
            Atime: entry.Header.AccessTime,
            Ctime: entry.Header.ChangeTime,
            Children: []Node{},
            Archive: archive,
        }

        if fileIsDirectory {
            node.Children = parseNodes(node.FullName, entries, archive)
            for _, child := range node.Children {
                child.Parent = &node
            }
        }

        nodes = append(nodes, node)
    }

    return nodes
}

func (node *Node) listRecursive(nodes *[]*Node) {
    *nodes = append(*nodes, node)
    for index, _ := range node.Children {
        node.Children[index].listRecursive(nodes)
    }
}

func (node *Node) IsLink() bool {
    return node.typeflag == tar.TypeLink
}

func (node *Node) IsSymlink() bool {
    return node.typeflag == tar.TypeSymlink
}

func (archive *Archive) List() []*Node {
    nodes := []*Node{}
    for index, _ := range archive.Nodes {
        archive.Nodes[index].listRecursive(&nodes)
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
    file, err := os.Open(node.Archive.Path)
    if err != nil {
        return nil, err
    }

    reader := tar.NewReader(file)
    for i := uint(0); i <= node.index; i++ {
        if _, err := reader.Next(); err != nil {
            return nil, err
        }
    }

    return &NodeReader{file: file, reader: reader}, nil
}
