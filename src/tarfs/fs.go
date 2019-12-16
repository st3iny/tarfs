package main

import (
    "io"
    _ "log"
    "os"
    "strconv"
    "syscall"
    "os/user"

    "bazil.org/fuse"
    "bazil.org/fuse/fs"
    "golang.org/x/net/context"
)

func MountAndServe(archivePath string, mountpoint string) error {
    c, err := fuse.Mount(
        mountpoint,
        fuse.FSName("tarfs"),
        fuse.Subtype("tarfs"),
        fuse.ReadOnly(),
    )
    if err != nil {
        return err
    }
    defer c.Close()

    srv := fs.New(c, nil)

    var archive *Archive
    archive, err = ReadArchive(archivePath)
    if err != nil {
        return err
    }

    filesys := &FS{
        Archive: *archive,
    }

    if err := srv.Serve(filesys); err != nil {
        return err
    }

    // check if the mount process has an error to report.
    <-c.Ready
    if err := c.MountError; err != nil {
        return err
    }
    return nil
}

type FS struct {
    Archive Archive
}

type File struct {
    Node Node
}

type FileHandle struct {
    File *File
    Reader io.ReadCloser
    Offset int64
}

var _ fs.FS = (*FS)(nil)
// var _ fs.Node = (*File)(nil)
// var _ fs.Handle = (*FileHandle)(nil)

func (f *FS) Root() (fs.Node, error) {
    uid := int64(0)
    gid := int64(0)

    user, err := user.Current()
    if err == nil {
        uid, _ = strconv.ParseInt(user.Uid, 10, 32)
        gid, _ = strconv.ParseInt(user.Gid, 10, 32)
    }

    rootNode := Node{
        Name: "root",
        FullName: "",
        Mode: os.ModeDir | 0555,
        Uid: int(uid),
        Gid: int(gid),
        Children: f.Archive.Nodes,
        Archive: &f.Archive,
    }
    return &File{Node: rootNode}, nil
}

var _ fs.NodeStringLookuper = (*File)(nil)

func (f *File) Attr(ctx context.Context, a *fuse.Attr) error {
    blocks := uint64(f.Node.Size) / 512
    if blocks % 512 > 0 {
        blocks++
    }

    a.Inode = uint64(f.Node.index)
    a.Size = uint64(f.Node.Size)
    a.Blocks = blocks
    a.Mode = f.Node.Mode
    a.Uid = uint32(f.Node.Uid)
    a.Gid = uint32(f.Node.Gid)
    a.Mtime = f.Node.Mtime
    a.Atime = f.Node.Atime
    a.Ctime = f.Node.Ctime
    return nil
}

func (f *File) Lookup(ctx context.Context, name string) (fs.Node, error) {
    for _, child := range f.Node.Children {
        if name == child.Name {
            return &File{Node: child}, nil
        }
    }

    return nil, fuse.ENOENT
}

var _ fs.HandleReadDirAller = (*File)(nil)

func (f *File) ReadDirAll(ctx context.Context) ([]fuse.Dirent, error) {
    entries := []fuse.Dirent{}

    for _, node := range f.Node.Children {
        entryType := fuse.DT_File
        if node.Mode.IsDir() {
            entryType = fuse.DT_Dir
        }

        entry := fuse.Dirent{
            Inode: uint64(node.index),
            Name: node.Name,
            Type: entryType,
        }

        entries = append(entries, entry)
    }

    return entries, nil
}

var _ fs.NodeOpener = (*File)(nil)

func (f *File) Open(ctx context.Context, req *fuse.OpenRequest, resp *fuse.OpenResponse) (fs.Handle, error) {
    if !req.Flags.IsReadOnly() {
        return nil, fuse.Errno(syscall.EACCES)
    }
    resp.Flags |= fuse.OpenKeepCache
    resp.Flags |= fuse.OpenNonSeekable

    reader, err := f.Node.Open()
    if err != nil {
        return nil, fuse.EIO
    }

    if f.Node.Mode.IsDir() {
        return f, nil
    } else {
        fh := &FileHandle{
            File: f,
            Reader: reader,
            Offset: 0,
        }
        return fh, nil
    }

}

var _ fs.HandleReader = (*FileHandle)(nil)

func (fh *FileHandle) Read(ctx context.Context, req *fuse.ReadRequest, resp *fuse.ReadResponse) error {
    if fh.Offset != req.Offset {
        return fuse.ENOTSUP
    }

    buf := make([]byte, req.Size)
    count, err := fh.Reader.Read(buf)
    if err != nil && err != io.EOF {
        return fuse.EIO
    }

    if count != req.Size {
        buf = buf[:count]
    }
    fh.Offset += int64(count)

    resp.Data = buf
    return nil
}

var _ fs.HandleReleaser = (*FileHandle)(nil)

func (fh *FileHandle) Release(ctx context.Context, req *fuse.ReleaseRequest) error {
    err := fh.Reader.Close()
    if err != nil {
        return fuse.EIO
    }

    return nil
}
