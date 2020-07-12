package archive

import (
    "archive/tar"
    "fmt"
    "io"
    "os"
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

// implements io.ReadCloser
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
