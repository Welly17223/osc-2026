extern crate alloc;

use core::{array, fmt::Write, str};

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::{display, schedule, uart};

use spin::{Once, RwLock};

pub mod byte_device;
pub mod file_describtor_table;
pub mod ramdisk;
pub mod tempfs;

pub use file_describtor_table::{FileDescribeError, FileDescribeTable};

pub type VNode = Arc<Vnode>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    PermissionDenied,
    NoSpace,
    InvalidInput,
    IoError,
    IsByteDevice,
    Unimplemented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekFrom {
    Start(u64),
    Current(i64),
    End(i64),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenFlags {
    pub create: bool,
    pub read: bool,
    pub write: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VnodeType {
    Directory,
    File,
    Mknod,
}

pub struct VnodeMetadata {
    pub types: VnodeType,
}

pub struct Vnode {
    pub metadata: VnodeMetadata,
    pub parent: RwLock<Weak<Vnode>>,
    pub item: Option<Box<dyn VnodeItem>>,
    pub mount: RwLock<Option<Mount>>,
}

pub struct Mount {
    pub root: Once<Arc<Vnode>>,
    pub parent: Once<Weak<Vnode>>,
    pub fs: Arc<dyn FileSystem>,
}

pub trait FileSystem: Send + Sync {
    fn name(&self) -> &str;
    fn setup_mount(&self, parent: Weak<Vnode>) -> Result<Arc<Vnode>, VfsError>;
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
pub trait VnodeLeaf: Send + Sync {}

impl<T> VnodeOps for T
where
    T: VnodeLeaf,
{
    fn lookup(&self, _parent: &Arc<Vnode>, _name: &str) -> Result<Arc<Vnode>, VfsError> {
        Err(VfsError::NotADirectory)
    }

    fn create(&self, _parent: &Arc<Vnode>, _name: &str) -> Result<Arc<Vnode>, VfsError> {
        Err(VfsError::NotADirectory)
    }

    fn mkdir(&self, _parent: &Arc<Vnode>, _name: &str) -> Result<Arc<Vnode>, VfsError> {
        Err(VfsError::NotADirectory)
    }

    fn list(&self) -> Result<Vec<(String, Arc<Vnode>)>, VfsError> {
        Err(VfsError::NotADirectory)
    }
}

#[derive(Clone)]
pub struct File {
    pub vnode: Arc<Vnode>,
    pub f_pos: u64,
    pub flags: OpenFlags,
}

impl File {
    pub fn new(vnode: Arc<Vnode>, flags: OpenFlags) -> Self {
        Self {
            vnode,
            f_pos: 0,
            flags,
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, VfsError> {
        if !self.flags.read {
            return Err(VfsError::PermissionDenied);
        }
        let data = self.vnode.item.as_ref().ok_or(VfsError::NotFound)?;
        let bytes_read = data.read(self.vnode.clone(), self.f_pos, buf)?;
        self.f_pos += bytes_read as u64;
        Ok(bytes_read)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, VfsError> {
        if !self.flags.write {
            return Err(VfsError::PermissionDenied);
        }
        let data = self.vnode.item.as_ref().ok_or(VfsError::NotFound)?;
        let bytes_written = data.write(self.vnode.clone(), self.f_pos, buf)?;
        self.f_pos += bytes_written as u64;
        Ok(bytes_written)
    }

    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64, VfsError> {
        let data = self.vnode.item.as_ref().ok_or(VfsError::NotFound)?;
        let new_pos = data.seek(self.vnode.clone(), self.f_pos, pos)?;
        self.f_pos = new_pos;
        Ok(new_pos)
    }

    pub fn ioctl(&self, request: usize, ptr: *mut ()) -> Result<(), VfsError> {
        let data = self.vnode.item.as_ref().ok_or(VfsError::NotFound)?;
        data.ioctl(request, ptr)
    }

    pub(crate) fn len(&self) -> Result<u64, VfsError> {
        let data = self.vnode.item.as_ref().ok_or(VfsError::NotFound)?;
        data.seek(self.vnode.clone(), 0, SeekFrom::End(0))
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let Some(data) = self.vnode.item.as_ref() else {
            return;
        };
        let _ = data.close(self.vnode.clone());
    }
}

impl SeekFrom {
    pub fn from_raw(offset: isize, whence: usize) -> Self {
        match whence {
            0 => Self::Start(offset as _),
            1 => Self::Current(offset as _),
            _ => Self::End(offset as _),
        }
    }
}

pub struct Vfs {
    rootfs: RwLock<Option<Arc<Mount>>>,
    filesystems: RwLock<BTreeMap<String, Arc<dyn FileSystem>>>,
}

pub static ROOT: Once<Vfs> = Once::new();

impl Vfs {
    pub fn new() -> Self {
        Self {
            rootfs: RwLock::default(),
            filesystems: RwLock::default(),
        }
    }

    pub fn register_filesystem(&self, fs: Arc<dyn FileSystem>) -> Result<(), VfsError> {
        let mut filesystems = self.filesystems.write();
        filesystems.insert(String::from(fs.name()), fs);
        Ok(())
    }

    pub fn lookup(&self, pathname: &str) -> Result<Arc<Vnode>, VfsError> {
        let rootfs = self.rootfs.read();
        let root = rootfs.as_ref().ok_or(VfsError::NotFound)?;

        let root_node = root
            .root
            .call_once(|| root.fs.setup_mount(Weak::default()).unwrap())
            .clone();

        if pathname == "/" {
            return Ok(root_node);
        }

        let mut current_node = if pathname.starts_with("/") {
            root_node
        } else {
            schedule::current_tcb().cwd.clone()
        };

        for curr_path in pathname.split("/") {
            match curr_path {
                "." | "" => (),
                ".." => {
                    let parent = current_node.parent.read();
                    if let Some(p) = parent.upgrade() {
                        drop(parent);
                        current_node = p;
                    };
                }
                _ => {
                    let mount = current_node.mount.read();
                    let temp = if let Some(child) = mount.as_ref() {
                        child.root.call_once(|| {
                            child
                                .fs
                                .setup_mount(child.parent.get().unwrap().clone())
                                .unwrap()
                        })
                    } else {
                        &current_node
                    }
                    .item
                    .as_ref()
                    .ok_or(VfsError::NotFound)?
                    .lookup(&current_node, curr_path)?
                    .clone();
                    drop(mount);

                    current_node = temp;
                }
            }
        }

        let target_node = current_node.clone();
        let mount_opt = target_node.mount.read();
        if let Some(mount) = mount_opt.as_ref() {
            let parent = current_node.parent.read().clone();
            current_node = mount
                .root
                .call_once(|| mount.fs.setup_mount(parent).unwrap())
                .clone();
        }

        Ok(current_node)
    }

    pub fn open(&self, pathname: &str, flags: OpenFlags) -> Result<File, VfsError> {
        let target_vnode = match self.lookup(pathname) {
            Ok(f) => f,
            Err(VfsError::NotFound) if flags.create => {
                let idx = pathname.rfind("/").unwrap_or(0);
                let parent_name = &pathname[..idx + 1];
                match self.mkdir(parent_name, true) {
                    Ok(_) | Err(VfsError::AlreadyExists) => (),
                    Err(e) => {
                        return Err(e);
                    }
                };
                self.create(pathname)?;
                self.lookup(pathname)?
            }
            Err(e) => return Err(e),
        };
        let item = target_vnode.item.as_ref().ok_or(VfsError::NotFound)?;
        item.open(target_vnode.clone())?;
        Ok(File::new(target_vnode, flags))
    }

    pub fn mkdir(&self, pathname: &str, recursive: bool) -> Result<(), VfsError> {
        if recursive {
            let rootfs = self.rootfs.read();
            let root = rootfs.as_ref().ok_or(VfsError::NotFound)?;

            let root_node = root
                .root
                .call_once(|| root.fs.setup_mount(Weak::default()).unwrap())
                .clone();

            let mut current_node = if pathname.starts_with("/") {
                root_node
            } else {
                schedule::current_tcb().cwd.clone()
            };

            for curr_path in pathname.split("/") {
                match curr_path {
                    "." | "" => (),
                    ".." => {
                        let parent = current_node.parent.read();
                        if let Some(p) = parent.upgrade() {
                            drop(parent);
                            current_node = p;
                        };
                    }
                    _ => {
                        let mount = current_node.mount.read();
                        let temp = if let Some(child) = mount.as_ref() {
                            child.root.call_once(|| {
                                child
                                    .fs
                                    .setup_mount(child.parent.get().unwrap().clone())
                                    .unwrap()
                            })
                        } else {
                            &current_node
                        }
                        .item
                        .as_ref()
                        .ok_or(VfsError::NotFound)?;

                        let child_node = match temp.mkdir(&current_node, curr_path) {
                            Ok(node) => node,
                            Err(VfsError::AlreadyExists) => {
                                temp.lookup(&current_node, curr_path)?
                            }
                            Err(e) => return Err(e),
                        };

                        drop(mount);

                        if child_node.metadata.types != VnodeType::Directory {
                            return Err(VfsError::NotADirectory);
                        }

                        current_node = child_node;
                    }
                }
            }
        } else {
            let idx = pathname.rfind("/").unwrap_or(0);
            let dir_name = &pathname[idx + 1..];
            let parent_name = &pathname[..idx + 1];

            let parent = self.lookup(parent_name)?;
            parent
                .item
                .as_ref()
                .ok_or(VfsError::NotFound)?
                .mkdir(&parent, dir_name)?;
        }

        Ok(())
    }

    pub fn create(&self, pathname: &str) -> Result<(), VfsError> {
        let file_name = pathname.split("/").last().unwrap_or(pathname);
        let idx = pathname.rfind("/").unwrap_or(0);

        let node = self.lookup(&pathname[..idx + 1])?;

        node.item
            .as_ref()
            .ok_or(VfsError::NotFound)?
            .create(&node, file_name)?;

        Ok(())
    }

    pub fn mount(&self, target: &str, fs_name: &str) -> Result<(), VfsError> {
        let filesystems = self.filesystems.read();
        let fs = filesystems.get(fs_name).ok_or(VfsError::NotFound)?.clone();

        if target == "/" {
            let mut root = self.rootfs.write();
            *root = Some(Arc::new(Mount {
                root: Once::default(),
                parent: Once::default(),
                fs,
            }));
            return Ok(());
        }

        let node = self.lookup(target)?;
        let node_parent = node.parent.read();
        let mut mount = node.mount.write();

        let set_up = Mount {
            root: Once::new(),
            parent: Once::new(),
            fs,
        };
        let _ = set_up.parent.call_once(|| node_parent.clone());

        *mount = Some(set_up);
        Ok(())
    }

    pub fn mknod(&self, target: &str, device: Box<dyn VnodeItem>) -> Result<(), VfsError> {
        let dir_name = target.split("/").last().unwrap_or(target);
        let idx = target.rfind("/").unwrap_or(0);

        let node = self.lookup(&target[..idx + 1])?;

        let item = node.item.as_ref().ok_or(VfsError::NotFound)?;
        item.mknod(&node, dir_name, device)?;

        Ok(())
    }

    pub fn root(&self) -> Result<Arc<Vnode>, VfsError> {
        self.lookup("/")
    }
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

pub const O_CREAT: usize = 0o100;

impl From<&str> for OpenFlags {
    fn from(value: &str) -> Self {
        let mut flags_o = Self::default();

        for s in value.bytes() {
            match s {
                b'r' => flags_o.read = true,
                b'w' => flags_o.write = true,
                b'c' => flags_o.create = true,
                _ => (),
            }
        }

        flags_o
    }
}

pub fn init_vfs() {
    let vfs = ROOT.call_once(Vfs::new);

    let tempfs = tempfs::Tmpfs::default();
    vfs.register_filesystem(Arc::new(tempfs)).unwrap();
    vfs.mount("/", "tmpfs").unwrap();

    let ramfs = ramdisk::RamFs {
        base_addr: unsafe { crate::ramdisk::INITRD_START },
    };
    vfs.register_filesystem(Arc::new(ramfs)).unwrap();
    vfs.mkdir("/dev", false).unwrap();
    vfs.mkdir("/ramfs", false).unwrap();
    vfs.mkdir("/test/baba/bb/dd", true).unwrap();
    // vfs.mount("/test", "tmpfs").unwrap();
    vfs.mount("/ramfs", "ramfs").unwrap();

    let uart_file = uart::UartInode {
        uart: RwLock::new(uart::get_serial()),
    };
    vfs.mknod("/dev/uart", Box::new(uart_file)).unwrap();

    let fb_file = *display::DISPLAY.get().unwrap();
    vfs.mknod("/dev/fb", Box::new(fb_file)).unwrap();

    // test_vfs();
}

pub fn test_vfs() {
    let serial = uart::get_serial();
    serial.disable_interrupt();

    let test_write_create = ["/dev/bala.txt", "/dev/baba/www.txt"];

    let root = ROOT.get().unwrap();
    writeln!(serial).unwrap();

    for filename in test_write_create {
        writeln!(serial, "{filename}:").unwrap();
        let mut bytes: [u8; 16] = array::from_fn(|idx| (idx as u8) * 2);
        let mut file = match root.open(filename, OpenFlags::from("rwc")) {
            Ok(f) => f,
            Err(e) => {
                writeln!(serial, "Open file {filename} error: {e:?}\n").unwrap();
                continue;
            }
        };

        let result = file.write(&bytes);
        match result {
            Ok(b) => {
                writeln!(serial, "read {b} bytes").unwrap();
                writeln!(serial, "contents:\n{:?}", bytes).unwrap();
            }
            Err(e) => writeln!(serial, "fail: {:?}", e).unwrap(),
        }

        file.seek(SeekFrom::Start(0)).unwrap();

        loop {
            bytes = [0u8; _];
            let result = file.read(&mut bytes);
            match result {
                Ok(0) => {
                    break;
                }
                Ok(b) => {
                    writeln!(serial, "read {b} bytes").unwrap();
                    writeln!(serial, "contents:\n{:?}", bytes).unwrap();
                }
                Err(e) => writeln!(serial, "fail: {:?}", e).unwrap(),
            }
        }
    }

    panic!("Normal Exit");
}
