use super::{Arc, FileOps, VfsError};

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
