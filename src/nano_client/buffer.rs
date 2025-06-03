pub(super) struct Buffer<'a> {
    buffer: &'a mut [u8],
    used: usize,
}

impl<'a> Buffer<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer, used: 0 }
    }

    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    pub fn reset(&mut self) {
        self.used = 0;
    }

    pub fn advance(&mut self, n: usize) {
        self.used += n;
    }

    pub fn used_slice(&self) -> &[u8] {
        &self.buffer[..self.used]
    }

    pub fn unused_slice_mut(&mut self) -> &mut [u8] {
        &mut self.buffer[self.used..]
    }

    pub fn remove_prefix(&mut self, len: usize) {
        if len >= self.used {
            self.used = 0;
            return;
        }
        self.buffer.copy_within(len..self.used, 0);
        self.used -= len;
    }
}
