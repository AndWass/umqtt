use core::ops::{Index, IndexMut};
use core::ops::Range;
use crate::packetview::WriteError;

pub struct BorrowedBuf<'a> {
    buffer: &'a mut [u8],
    len: usize,
}

impl<'a> BorrowedBuf<'a> {
    fn unwritten_slice(&mut self, len: usize) -> &mut [u8] {
        &mut self.buffer[self.len..(self.len + len)]
    }
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            len: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    pub fn spare_capacity(&self) -> usize {
        self.capacity() - self.len
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn add_slice(&mut self, data: &[u8]) -> Result<(), WriteError> {
        let spare_capacity = self.spare_capacity();
        if data.len() <= spare_capacity {
            self.unwritten_slice(data.len()).copy_from_slice(data);
            self.len += data.len();
            Ok(())
        }
        else {
            Err(WriteError::NotEnoughCapacity)
        }
    }
}

impl<'a> Index<usize> for BorrowedBuf<'a> {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.buffer[index]
    }
}

impl<'a> IndexMut<usize> for BorrowedBuf<'a> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.buffer[index]
    }
}

impl<'a> Index<Range<usize>> for BorrowedBuf<'a> {
    type Output = [u8];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        &self.buffer[index]
    }
}

impl<'a> IndexMut<Range<usize>> for BorrowedBuf<'a> {
    fn index_mut(&mut self, index: Range<usize>) -> &mut Self::Output {
        &mut self.buffer[index]
    }
}
