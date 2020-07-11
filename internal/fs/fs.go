package fs

import (
    "fmt"
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

func MountAndServe(archivePath string, mountpoint string, debug bool) error {
    if debug {
        fuse.Debug = func(msg interface{}) {
            log.Println(msg)
        }
    }

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

    user, err := user.Current()
    if err != nil {
        return nil, err
    }
    uid, err1 := strconv.Atoi(user.Uid)
    gid, err2 := strconv.Atoi(user.Gid)
    if err1 != nil || err2 != nil {
        return nil, fmt.Errorf("Could not get user uid or gid")
    }

    root := &archive.Node{
        Name: "root",
        FullName: "",
        Mode: os.ModeDir | 0o555,
        Uid: uid,
        Gid: gid,
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

func (fs *FS) Root() (fs.Node, error) {
    return fs.RootNode, nil
}

func (f File) Attr(ctx context.Context, a *fuse.Attr) error {
    if f.Node.IsLink() {
        return f.FS.LinkMap[f.Node.LinkName].Attr(ctx, a)
    }

    a.Size = uint64(f.Node.Size)
    a.Mode = f.Node.Mode
    a.Uid = uint32(f.Node.Uid)
    a.Gid = uint32(f.Node.Gid)
    a.Mtime = f.Node.Mtime
    a.Atime = f.Node.Atime
    a.Ctime = f.Node.Ctime
    return nil
}

func (f File) Lookup(ctx context.Context, name string) (fs.Node, error) {
    for _, child := range f.Node.Children {
        if name == child.Name {
            return File{Node: &child, FS: f.FS}, nil
        }
    }

    return nil, fuse.ENOENT
}

func (f File) ReadDirAll(ctx context.Context) ([]fuse.Dirent, error) {
    entries := make([]fuse.Dirent, 0, len(f.Node.Children))

    for _, node := range f.Node.Children {
        entryType := fuse.DT_File
        if node.Mode.IsDir() {
            entryType = fuse.DT_Dir
        }

        entry := fuse.Dirent{
            Name: node.Name,
            Type: entryType,
        }

        entries = append(entries, entry)
    }

    return entries, nil
}

func (f File) Readlink(ctx context.Context, req *fuse.ReadlinkRequest) (string, error) {
    return f.Node.LinkName, nil
}

func (f File) Open(ctx context.Context, req *fuse.OpenRequest, resp *fuse.OpenResponse) (fs.Handle, error) {
    if !req.Flags.IsReadOnly() {
        return nil, fuse.Errno(syscall.EACCES)
    }

    if f.Node.Mode.IsDir() {
        return f, nil
    } else if f.Node.IsLink() {
        return f.FS.LinkMap[f.Node.LinkName].Open(ctx, req, resp)
    }

    resp.Flags |= fuse.OpenKeepCache
    resp.Flags |= fuse.OpenNonSeekable

    reader, err := f.Node.Open()
    if err != nil {
        return nil, fuse.EIO
    }

    fh := &FileHandle{
        File: &f,
        Reader: reader,
        Offset: 0,
    }
    return fh, nil
}

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

func (fh *FileHandle) Release(ctx context.Context, req *fuse.ReleaseRequest) error {
    err := fh.Reader.Close()
    if err != nil {
        return fuse.EIO
    }

    return nil
}
