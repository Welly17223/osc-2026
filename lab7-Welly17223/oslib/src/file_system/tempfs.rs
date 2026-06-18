use core::cmp::{max, min};

use super::{
    Arc, BTreeMap, Box, RwLock, String, Vec, VfsError, Vnode, VnodeMetadata, VnodeType, Weak,
};

#[derive(Default)]
pub struct Tmpfs {}

const TEMPFS_FILE_SIZE_LIMIT: usize = 0x1000;

enum TempfsContent {
    Directory(BTreeMap<String, Arc<Vnode>>),
    File {
        ref_count: usize,
        content: Box<[u8; TEMPFS_FILE_SIZE_LIMIT]>,
        curr_size: usize,
    },
}

struct TempfsInode {
    inode: RwLock<TempfsContent>,
}

impl super::FileSystem for Tmpfs {
    fn name(&self) -> &str {
        "tmpfs"
    }

    fn setup_mount(&self, parent: Weak<super::Vnode>) -> Result<Arc<super::Vnode>, VfsError> {
        Ok(Arc::new(super::Vnode {
            metadata: VnodeMetadata {
                types: VnodeType::Directory,
            },
            parent: RwLock::new(parent.clone()),
            item: Some(Box::new(TempfsInode {
                inode: RwLock::new(TempfsContent::new_dir()),
            })),
            mount: RwLock::default(),
        }))
    }
}

impl super::VnodeItem for TempfsInode {}

impl super::VnodeOps for TempfsInode {
    fn lookup(
        &self,
        _parent: &Arc<super::Vnode>,
        name: &str,
    ) -> Result<Arc<super::Vnode>, VfsError> {
        let inode = self.inode.read();
        match &*inode {
            TempfsContent::Directory(d) => Ok(d.get(name).ok_or(VfsError::NotFound)?.clone()),
            TempfsContent::File {
                ref_count: _,
                content: _,
                curr_size: _,
            } => Err(VfsError::NotADirectory),
        }
    }

    fn create(
        &self,
        parent: &Arc<super::Vnode>,
        name: &str,
    ) -> Result<Arc<super::Vnode>, VfsError> {
        let mut inode = self.inode.write();
        inode.check_new_file(name)?;

        match &mut *inode {
            TempfsContent::Directory(dir) => {
                if dir.contains_key(name) {
                    return Err(VfsError::AlreadyExists);
                }

                let child = Arc::new(super::Vnode {
                    metadata: VnodeMetadata {
                        types: VnodeType::File,
                    },
                    parent: RwLock::new(Arc::downgrade(parent)),
                    item: Some(Box::new(TempfsInode {
                        inode: RwLock::new(TempfsContent::new_file()),
                    })),
                    mount: RwLock::default(),
                });
                dir.insert(String::from(name), child.clone());
                Ok(child)
            }
            TempfsContent::File {
                ref_count: _,
                content: _,
                curr_size: _,
            } => Err(VfsError::NotADirectory),
        }
    }

    fn mkdir(&self, parent: &Arc<super::Vnode>, name: &str) -> Result<Arc<super::Vnode>, VfsError> {
        let mut inode = self.inode.write();

        inode.check_new_file(name)?;

        match &mut *inode {
            TempfsContent::Directory(dir) => {
                let child = Arc::new(super::Vnode {
                    metadata: VnodeMetadata {
                        types: VnodeType::Directory,
                    },
                    parent: RwLock::new(Arc::downgrade(parent)),
                    item: Some(Box::new(TempfsInode {
                        inode: RwLock::new(TempfsContent::new_dir()),
                    })),
                    mount: RwLock::default(),
                });
                dir.insert(String::from(name), child.clone());
                Ok(child)
            }
            TempfsContent::File {
                ref_count: _,
                content: _,
                curr_size: _,
            } => Err(VfsError::NotADirectory),
        }
    }

    fn list(&self) -> Result<Vec<(String, Arc<Vnode>)>, VfsError> {
        let inode = self.inode.read();
        match &*inode {
            TempfsContent::Directory(dir) => Ok(dir
                .iter()
                .map(|(name, node)| (name.clone(), node.clone()))
                .collect()),
            TempfsContent::File {
                ref_count: _,
                content: _,
                curr_size: _,
            } => Err(VfsError::IsADirectory),
        }
    }

    fn mknod(
        &self,
        parent: &Arc<Vnode>,
        name: &str,
        dev: Box<dyn super::VnodeItem>,
    ) -> Result<Arc<Vnode>, VfsError> {
        let mut inode = self.inode.write();
        match &mut *inode {
            TempfsContent::Directory(dir) => {
                let child = Arc::new(Vnode {
                    metadata: VnodeMetadata {
                        types: VnodeType::Mknod,
                    },
                    parent: RwLock::new(Arc::downgrade(parent)),
                    item: Some(dev),
                    mount: RwLock::new(None),
                });
                dir.insert(String::from(name), child.clone());
                Ok(child)
            }
            TempfsContent::File {
                ref_count: _,
                content: _,
                curr_size: _,
            } => Err(VfsError::IsADirectory),
        }
    }
}

impl super::FileOps for TempfsInode {
    fn open(&self, _vnode: Arc<Vnode>) -> Result<(), VfsError> {
        let mut inode = self.inode.write();
        match &mut *inode {
            TempfsContent::Directory(_) => Err(VfsError::IsADirectory),
            TempfsContent::File {
                ref_count,
                content: _,
                curr_size: _,
            } => {
                *ref_count += 1;
                Ok(())
            }
        }
    }

    fn read(&self, _vnode: Arc<Vnode>, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let inode = self.inode.read();

        match &*inode {
            TempfsContent::Directory(_) => Err(VfsError::IsADirectory),
            TempfsContent::File {
                ref_count: _,
                content,
                curr_size,
            } => {
                let offset = offset as usize;
                let buf_len = buf.len();
                let read_end = min(offset + buf_len, *curr_size);
                buf[0..(read_end - offset)].copy_from_slice(&content[offset..read_end]);
                Ok(read_end - offset)
            }
        }
    }

    fn write(&self, _vnode: Arc<Vnode>, offset: u64, buf: &[u8]) -> Result<usize, VfsError> {
        let mut inode = self.inode.write();

        match &mut *inode {
            TempfsContent::Directory(_) => Err(VfsError::IsADirectory),
            TempfsContent::File {
                ref_count: _,
                content,
                curr_size,
            } => {
                let offset = offset as usize;
                let buf_len = buf.len();
                let write_end = min(offset + buf_len, content.len());
                *curr_size = max(write_end, *curr_size);

                content[offset..write_end].copy_from_slice(&buf[0..(write_end - offset)]);

                Ok(write_end - offset)
            }
        }
    }

    fn seek(&self, _vnode: Arc<Vnode>, f_pos: u64, pos: super::SeekFrom) -> Result<u64, VfsError> {
        let mut inode = self.inode.write();

        match &mut *inode {
            TempfsContent::Directory(_) => Err(VfsError::IsADirectory),
            TempfsContent::File {
                ref_count: _,
                content: _,
                curr_size,
            } => {
                let mut n = match pos {
                    super::SeekFrom::Start(o) => o,
                    super::SeekFrom::Current(o) => f_pos.saturating_add_signed(o),
                    super::SeekFrom::End(o) => (*curr_size as u64).saturating_add_signed(o),
                };

                n = min(n, *curr_size as _);
                Ok(n)
            }
        }
    }
}

impl TempfsContent {
    fn new_dir() -> Self {
        Self::Directory(BTreeMap::default())
    }

    fn new_file() -> Self {
        Self::File {
            ref_count: 0,
            content: Box::new([0; TEMPFS_FILE_SIZE_LIMIT]),
            curr_size: 0,
        }
    }

    fn check_new_file(&self, name: &str) -> Result<(), VfsError> {
        match self {
            Self::Directory(dir) => {
                if name.len() >= 16 || dir.len() >= 16 {
                    return Err(VfsError::IoError);
                }

                if dir.contains_key(name) {
                    return Err(VfsError::AlreadyExists);
                }
            }
            Self::File {
                ref_count: _,
                content: _,
                curr_size: _,
            } => (),
        }
        Ok(())
    }
}
