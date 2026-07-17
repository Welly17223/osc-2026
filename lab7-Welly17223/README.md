# Lab 7 Virtual File System

上課講義：[File system](https://people.cs.nycu.edu.tw/~ttyeh/course/2026_Spring/IOC5226/slide/file-I.pdf)、[Journal file system](https://people.cs.nycu.edu.tw/~ttyeh/course/2026_Spring/IOC5226/slide/file-II.pdf)

[Exercise](https://reurl.cc/YD537O)

目標是實做 Virtual file system 提供實體 file system 實做的界面，由於不涉硬體操作，因此相對簡單。

![](https://nycu-caslab.github.io/OSC2026/_images/lab7_impl_vis.png)

## Basic Exercise

基本上所有的節點都是不可變的，如果需要改變內部狀態則是使用擁有內部可變性的 `RwLock`、`Mutex` 等來實現。

### `vnode`

不管是 Directory、Mount Point 或是 File 等在 `vfs` 裡面都是一個 `vnode`，透過存取 `vnode` 裡面的 `item` 或是 `mount` field 來真正的讀取內部儲存的內容。實做 `VnodeItem` 包含檔案操作如 `read` 或是 `write` 以及資料夾操作如 `mkdir`、`lookup` 等。`metadata` 應該是用來擁有者以及儲存權限之類的，不過作業沒有要求，因此只用來儲存這個 `Vnode` 是 `Director`、`File`或是 `Mknod` 這樣的 device file 。

```rust
pub struct Vnode {
    pub metadata: VnodeMetadata,
    pub parent: RwLock<Weak<Vnode>>,
    pub item: Option<Box<dyn VnodeItem>>,
    pub mount: RwLock<Option<Mount>>,
}

pub trait FileOps: Send + Sync {
    fn open(&self, _vnode: Arc<Vnode>) -> Result<(), VfsError> {
        Ok(())
    }

    fn close(&self, _vnode: Arc<Vnode>) -> Result<(), VfsError> {
        Ok(())
    }

    fn read(&self, vnode: Arc<Vnode>, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&self, vnode: Arc<Vnode>, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
    fn seek(&self, vnode: Arc<Vnode>, f_pos: u64, pos: SeekFrom) -> Result<u64, VfsError>;
    fn ioctl(&self, _request: usize, _ptr: *mut ()) -> Result<(), VfsError> {
        Err(VfsError::Unimplemented)
    }
}

pub trait VnodeOps: Send + Sync {
    fn lookup(&self, parent: &Arc<Vnode>, name: &str) -> Result<Arc<Vnode>, VfsError>;
    fn create(&self, parent: &Arc<Vnode>, name: &str) -> Result<Arc<Vnode>, VfsError>;
    fn mkdir(&self, parent: &Arc<Vnode>, name: &str) -> Result<Arc<Vnode>, VfsError>;
    fn list(&self) -> Result<Vec<(String, Arc<Vnode>)>, VfsError>;
    fn mknod(
        &self,
        _parent: &Arc<Vnode>,
        _name: &str,
        _dev: Box<dyn VnodeItem>,
    ) -> Result<Arc<Vnode>, VfsError> {
        Err(VfsError::PermissionDenied)
    }
}

pub trait VnodeItem: VnodeOps + FileOps {}
```

### `Mount`

負責掛載文件系統的節點， `root` 儲存了此檔案系統的根目錄，而 `fs` 則是儲存負責初始化這個 mount point 的函數。

```rust
pub struct Mount {
    pub root: Once<Arc<Vnode>>,
    pub parent: Once<Weak<Vnode>>,
    pub fs: Arc<dyn FileSystem>,
}

pub trait FileSystem: Send + Sync {
    fn name(&self) -> &str;
    fn setup_mount(&self, parent: Weak<Vnode>) -> Result<Arc<Vnode>, VfsError>;
}
```

### `File`

描述一個開啟的檔案，負責管理目前的讀寫頭位置、檔案開啟的 flags 以及呼叫 `Vnode` 裡面的 `read`、`write` 等 method 。

### `Vfs`

用來當作 `VFS` 的 root 的 struct ，實做了 `vfs` 需要提供的一些 method 包含 `register_filesystem`、`lookup`、`open`、`mkdir`、`create`、`mount`、`mknod` 等。

```rust
pub struct Vfs {
    rootfs: RwLock<Option<Arc<Mount>>>,
    filesystems: RwLock<BTreeMap<String, Arc<dyn FileSystem>>>,
}
```

### `tmpfs`

根據前面提供的 `VnodeItem` 來實做內部的方法，基本上沒有什麼困難的。

### File Descriptor Table

作業規格是最多有 16 個 file descriptor ，因此長度為 16 。負責管理 process 開啟的檔案，其中遵循 POSIX 標準，0 為 `stdin`，1 為 `stdout`，`2` 為 `stderr`。

```rust
#[derive(Clone, Default)]
pub struct FileDescribeTable {
    valid_bits: u16,
    fds: [Option<File>; 16],
}
```

### System call

實做跟檔案相關的 system call ，就只是呼叫 `vfs` 的 method 而已，比較需要注意的是 `O_CREAT` 這一個 flags 是當檔案不存在或是資料夾不存在都需要建立。

## `/ramfs`

實做 `ramfs` 這個 file system ，除了寫入操作要回傳錯誤以外，內容與 `tempfs` 基本相同。

## `/dev/uart` and `/dev/fb`

我的實做方法是提供 `ByteDevice` 的 trait ，並且由 `mknod` 來建立相關的 File 。

```rust
pub trait ByteDevice: Sync + Send {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
    fn ioctl(&self, _requests: usize, _ptr: *mut ()) -> Result<(), VfsError> {
        Err(VfsError::Unimplemented)
    }
    fn seek(
        &self,
        _vnode: Arc<super::Vnode>,
        _f_pos: u64,
        _pos: super::SeekFrom,
    ) -> Result<u64, VfsError> {
        Err(VfsError::IsByteDevice)
    }
}

impl<T: ByteDevice> FileOps for T {
    fn read(
        &self,
        _vnode: Arc<super::Vnode>,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, VfsError> {
        self.read(offset, buf)
    }

    fn write(&self, _vnode: Arc<super::Vnode>, offset: u64, buf: &[u8]) -> Result<usize, VfsError> {
        self.write(offset, buf)
    }

    fn seek(
        &self,
        vnode: Arc<super::Vnode>,
        f_pos: u64,
        pos: super::SeekFrom,
    ) -> Result<u64, VfsError> {
        self.seek(vnode, f_pos, pos)
    }

    fn ioctl(&self, request: usize, ptr: *mut ()) -> Result<(), VfsError> {
        self.ioctl(request, ptr)
    }
}

impl<T: ByteDevice> super::VnodeLeaf for T {}
impl<T: ByteDevice> super::VnodeItem for T {}
```

## 心得

雖然檔案系統的設計有很多需要思考以及抽象的，不過助教都有提供，因此寫起來應該相比前面的作業來說相當容易，筆記也沒有什麼好寫的，除了將 C 的 function 轉換成 rust 的 trait 比較需要花時間以外，很多都是 trivial code ，而且也不用支援刪除操作，因此感覺確實沒有甚麼好說的。
