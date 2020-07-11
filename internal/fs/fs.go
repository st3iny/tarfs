package fs

import (
    "io"
    "log"
    "os"
    "strconv"
    "syscall"
    "os/user"

    "github.com/st3iny/tarfs/internal/archive"

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

    filesys, err := createFS(archivePath)
    if err != nil {
        return err
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

func createFS(archivePath string) (*FS, error) {
    var arch *archive.Archive
    arch, err := archive.ReadArchive(archivePath)
    if err != nil {
        return nil, err
    }

    uid := int64(0)
    gid := int64(0)

    user, err := user.Current()
    if err == nil {
        uid, _ = strconv.ParseInt(user.Uid, 10, 32)
        gid, _ = strconv.ParseInt(user.Gid, 10, 32)
    }

    root := &archive.Node{
        Name: "root",
        FullName: "",
        Mode: os.ModeDir | 0555,
        Uid: int(uid),
        Gid: int(gid),
        Children: arch.Nodes,
        Archive: arch,
    }

    filesys := &FS{
        Archive: *arch,
        RootNode: File{Node: root},
    }

    linkMap := createLinkMap(arch, filesys)
    filesys.LinkMap = linkMap
    filesys.RootNode.FS = filesys

    return filesys, nil
}

func createLinkMap(archive *archive.Archive, filesys *FS) map[string]*File  {
    linkMap := make(map[string]*File)
    for _, node := range archive.List() {
        if node.IsLink() {
            linkMap[node.LinkName] = nil
        }
    }

    for _, node := range archive.List() {
        if _, present := linkMap[node.FullName]; present {
            file := &File{
                Node: node,
                FS: filesys,
            }
            linkMap[node.FullName] = file
        }
    }

    log.Println("found", len(linkMap), "hardlinks in archive")
    return linkMap
}

type FS struct {
    Archive archive.Archive
    RootNode File
    LinkMap map[string]*File
}

type File struct {
    Node *archive.Node
    FS *FS
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
    return &f.RootNode, nil
}

var _ fs.NodeStringLookuper = (*File)(nil)

func (f *File) Attr(ctx context.Context, a *fuse.Attr) error {
    blocks := uint64(f.Node.Size) / 512
    if blocks % 512 > 0 {
        blocks++
    }

    if f.Node.IsLink() {
        return f.FS.LinkMap[f.Node.LinkName].Attr(ctx, a)
    }

    a.Inode = uint64(f.Node.Index)
    if f.Node.IsSymlink() {
        a.Size = uint64(len(f.Node.LinkName))
    } else {
        a.Size = uint64(f.Node.Size)
    }
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
    for index, _ := range f.Node.Children {
        child := &f.Node.Children[index]
        if name == child.Name {
            return &File{Node: child, FS: f.FS}, nil
        }
    }

    return nil, fuse.ENOENT
}

var _ fs.HandleReadDirAller = (*File)(nil)

func (f *File) ReadDirAll(ctx context.Context) ([]fuse.Dirent, error) {
    entries := make([]fuse.Dirent, 0, len(f.Node.Children))

    for _, node := range f.Node.Children {
        entryType := fuse.DT_File
        if node.Mode.IsDir() {
            entryType = fuse.DT_Dir
        }

        entry := fuse.Dirent{
            Inode: uint64(node.Index),
            Name: node.Name,
            Type: entryType,
        }

        entries = append(entries, entry)
    }

    return entries, nil
}

var _ fs.NodeReadlinker = (*File)(nil)

func (f *File) Readlink(ctx context.Context, req *fuse.ReadlinkRequest) (string, error) {
    return f.Node.LinkName, nil
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
    } else if f.Node.IsLink() {
        return f.FS.LinkMap[f.Node.LinkName].Open(ctx, req, resp)
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
