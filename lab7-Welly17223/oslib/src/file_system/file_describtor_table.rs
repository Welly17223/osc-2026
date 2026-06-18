use crate::file_system::ROOT;

use super::File;
use core::ops::{Index, IndexMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDescribeError {
    OpenFileLimit,
    NotOpenFile,
    IndexOverflow,
    VfsError(super::VfsError),
}

impl From<super::VfsError> for FileDescribeError {
    fn from(value: super::VfsError) -> Self {
        Self::VfsError(value)
    }
}

#[derive(Clone, Default)]
pub struct FileDescribeTable {
    valid_bits: u16,
    fds: [Option<File>; 16],
}

impl Index<usize> for FileDescribeTable {
    type Output = Option<File>;
    fn index(&self, index: usize) -> &Self::Output {
        &self.fds[index]
    }
}

impl IndexMut<usize> for FileDescribeTable {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.fds[index]
    }
}

impl FileDescribeTable {
    pub fn open(
        &mut self,
        path: &str,
        flags: super::OpenFlags,
    ) -> Result<usize, FileDescribeError> {
        if !self.valid_bits == 0 {
            return Err(FileDescribeError::OpenFileLimit);
        }
        let free_descriptor = self.valid_bits.trailing_ones() as usize;

        let vfs_root = ROOT.get().unwrap();
        let file_node = vfs_root.open(path, flags)?;

        self[free_descriptor] = Some(file_node);
        self.valid_bits |= 1 << free_descriptor;
        Ok(free_descriptor)
    }

    pub fn close(&mut self, descriptor: usize) -> Result<(), FileDescribeError> {
        if descriptor >= 16 {
            return Err(FileDescribeError::IndexOverflow);
        }

        let file = self[descriptor]
            .take()
            .ok_or(FileDescribeError::NotOpenFile)?;

        file.vnode
            .item
            .as_ref()
            .ok_or(super::VfsError::NotFound)?
            .close(file.vnode.clone())?;

        self.valid_bits &= !(1 << descriptor);

        Ok(())
    }
}
