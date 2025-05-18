use crate::packetview::WriteError;
use core::ops::{Index, IndexMut};

#[derive(Clone)]
pub struct Cursor<'a>(pub &'a [u8]);

impl<'a> Cursor<'a> {
    pub fn as_slice(&self) -> &'a [u8] {
        self.0
    }
    pub fn advance(&mut self, n: usize) {
        self.0 = &self.0[n..];
    }

    pub fn take_u8(&mut self) -> u8 {
        let ret = self[0];
        self.advance(1);
        ret
    }

    pub fn take_u16(&mut self) -> u16 {
        let arr = [self[0], self[1]];
        self.advance(2);
        u16::from_be_bytes(arr)
    }

    pub fn split_to(&mut self, at: usize) -> &'a [u8] {
        let ret = &self.0[0..at];
        self.advance(at);
        ret
    }
}

impl Index<usize> for Cursor<'_> {
    type Output = u8;
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

pub struct WriteCursor<'a> {
    buffer: &'a mut [u8],
    next_write_index: usize,
}

impl<'a> WriteCursor<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            next_write_index: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    pub fn remaining_capacity(&self) -> usize {
        self.capacity() - self.bytes_written()
    }

    pub fn bytes_written(&self) -> usize {
        self.next_write_index
    }

    pub fn written_slice(&self) -> &[u8] {
        &self.buffer[0..self.next_write_index]
    }
    pub fn finish(self) -> (&'a [u8], &'a [u8]) {
        let initalized = &self.buffer[0..self.next_write_index];
        let remaining = &self.buffer[self.next_write_index..];
        (initalized, remaining)
    }

    pub fn put_u8(&mut self, byte: u8) -> Result<(), WriteError> {
        if self.next_write_index == self.capacity() {
            Err(WriteError::NotEnoughCapacity)
        } else {
            self.buffer[self.next_write_index] = byte;
            self.next_write_index += 1;
            Ok(())
        }
    }

    pub fn put_u16(&mut self, value: u16) -> Result<(), WriteError> {
        if self.remaining_capacity() < 2 {
            Err(WriteError::NotEnoughCapacity)
        } else {
            let value = value.to_be_bytes();
            self.put_slice(value.as_slice())
        }
    }

    pub fn put_slice(&mut self, slice: &[u8]) -> Result<(), WriteError> {
        if self.remaining_capacity() < slice.len() {
            Err(WriteError::NotEnoughCapacity)
        } else {
            self.buffer[self.next_write_index..self.next_write_index + slice.len()]
                .copy_from_slice(slice);
            self.next_write_index += slice.len();
            Ok(())
        }
    }

    pub fn advance(&mut self, n: usize) {
        self.next_write_index += n;
    }

    pub fn unused_capacity(&mut self) -> &mut [u8] {
        &mut self.buffer[self.next_write_index..]
    }
}

impl Index<usize> for WriteCursor<'_> {
    type Output = u8;
    fn index(&self, index: usize) -> &Self::Output {
        &self.buffer[index]
    }
}

impl IndexMut<usize> for WriteCursor<'_> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.buffer[index]
    }
}
