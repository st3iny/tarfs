package archive

import (
    "archive/tar"
    "fmt"
    "io"
    "os"
    "path"
    "strings"
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

    var entries []*tarEntry
    arch := &Archive{Path: path, compression: getCompression(file)}
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
    reader, err := decompress(file, arch.compression)
    if err != nil {
        return nil, err
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
