use core::cmp::min;

use crate::{file_system::VnodeItem, ramdisk};

use super::{
    Arc, BTreeMap, Box, FileOps, FileSystem, RwLock, String, Vec, VfsError, VnodeMetadata,
    VnodeOps, VnodeType, Weak,
};

struct Content<'a> {
    name: &'a str,
    data: &'a [u8],
}

struct Dir {
    pub base_addr: usize,
    pub data: RwLock<BTreeMap<String, Arc<super::Vnode>>>,
}

pub struct RamFs {
    pub base_addr: usize,
}

impl FileSystem for RamFs {
    fn name(&self) -> &str {
        "ramfs"
    }

    fn setup_mount(
        &self,
        parent: Weak<super::Vnode>,
    ) -> Result<Arc<super::Vnode>, super::VfsError> {
        let mut data = BTreeMap::new();
        let iter = ramdisk::CpioIter::new(self.base_addr as _);

        for (name, file) in iter {
            let vnode = Arc::new(super::Vnode {
                metadata: VnodeMetadata {
                    types: VnodeType::File,
                },
                parent: RwLock::default(),
                item: Some(Box::new(Content { name, data: file })),
                mount: RwLock::default(),
            });
            data.insert(String::from(name), vnode);
        }

        let dir_vnode = Arc::new(super::Vnode {
            metadata: VnodeMetadata {
                types: VnodeType::Directory,
            },
            parent: RwLock::new(parent),
            mount: RwLock::default(),
            item: Some(Box::new(Dir {
                base_addr: self.base_addr,
                data: RwLock::new(data.clone()),
            })),
        });

        for (_, node) in data {
            let mut node = node.parent.write();
            *node = Arc::downgrade(&dir_vnode);
        }

        Ok(dir_vnode)
    }
}

impl<'a> VnodeItem for Content<'a> {}
impl<'a> super::VnodeLeaf for Content<'a> {}

impl<'a> FileOps for Content<'a> {
    fn read(
        &self,
        _vnode: Arc<super::Vnode>,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, VfsError> {
        let offset = offset as usize;
        let buf_len = buf.len();
        let read_end = min(offset + buf_len, self.data.len());
        buf[0..(read_end - offset)].copy_from_slice(&self.data[offset..read_end]);
        Ok(read_end - offset)
    }

    fn write(
        &self,
        _vnode: Arc<super::Vnode>,
        _offset: u64,
        _buf: &[u8],
    ) -> Result<usize, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn seek(
        &self,
        _vnode: Arc<super::Vnode>,
        f_pos: u64,
        pos: super::SeekFrom,
    ) -> Result<u64, VfsError> {
        let mut n = match pos {
            super::SeekFrom::Start(o) => o,
            super::SeekFrom::Current(o) => f_pos.saturating_add_signed(o),
            super::SeekFrom::End(o) => (self.data.len() as u64).saturating_add_signed(o),
        };

        if n > self.data.len() as _ {
            n = self.data.len() as _;
        }

        Ok(n)
    }
}

impl VnodeItem for Dir {}

impl VnodeOps for Dir {
    fn lookup(
        &self,
        _parent: &Arc<super::Vnode>,
        name: &str,
    ) -> Result<Arc<super::Vnode>, VfsError> {
        let index = self.data.read();
        Ok(index.get(name).ok_or(VfsError::NotFound)?.clone())
    }

    fn create(
        &self,
        _parent: &Arc<super::Vnode>,
        _name: &str,
    ) -> Result<Arc<super::Vnode>, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn mkdir(
        &self,
        _parent: &Arc<super::Vnode>,
        _name: &str,
    ) -> Result<Arc<super::Vnode>, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn list(&self) -> Result<Vec<(String, Arc<super::Vnode>)>, VfsError> {
        let data = self.data.read();
        Ok(data
            .iter()
            .map(|(name, node)| (name.clone(), node.clone()))
            .collect())
    }
}

impl FileOps for Dir {
    fn open(&self, _vnode: Arc<super::Vnode>) -> Result<(), VfsError> {
        Err(VfsError::IsADirectory)
    }

    fn read(
        &self,
        _vnode: Arc<super::Vnode>,
        _offset: u64,
        _buf: &mut [u8],
    ) -> Result<usize, VfsError> {
        Err(VfsError::IsADirectory)
    }

    fn write(
        &self,
        _vnode: Arc<super::Vnode>,
        _offset: u64,
        _buf: &[u8],
    ) -> Result<usize, VfsError> {
        Err(VfsError::IsADirectory)
    }

    fn seek(
        &self,
        _vnode: Arc<super::Vnode>,
        _f_pos: u64,
        _pos: super::SeekFrom,
    ) -> Result<u64, VfsError> {
        Err(VfsError::IsADirectory)
    }
}
