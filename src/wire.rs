use std::{ops::{Add, AddAssign, Index, IndexMut, Sub}, ptr::NonNull};

pub fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ (-((n & 1) as i64))
}

pub fn zigzag_encode(n: i64) -> u64 {
    ((n as u64) << 1) ^ ((n >> 63) as u64)
}

#[derive(Clone, Copy)]
pub struct ReadCursor(NonNull<u8>);

impl ReadCursor {
    pub fn new(buffer: &[u8]) -> (Self, NonNull<u8>) {
        let ptr = ReadCursor(NonNull::from_ref(&buffer[0]));
        let end = ptr.limit(buffer.len() as isize);
        (ptr, end)
    }

    pub fn limit(&self, len: isize) -> NonNull<u8> {
        (*self + len).0
    }

    pub fn read_varint(&mut self) -> Option<u64> {
        let mut result = 0;
        let mut extra = 0;
        for i in 0..10 {
            let b = self[i];
            if i == 9 && b != 1 {
                return None;
            }
            result ^= (b as u64) << (7 * i);
            if b < 0x80 {
                *self += i + 1;
                return Some(result ^ extra);
            }
            extra ^= 0x80 << (7 * i);
        }
        None
    }

    pub fn read_tag(&mut self) -> Option<u32> {
        let mut result = 0;
        let mut extra = 0;
        for i in 0..5 {
            let b = self[i];
            if i == 4 && (b == 0 || b > 15) {
                return None;
            }
            result ^= (b as u32) << (7 * i);
            if b < 0x80 {
                *self += i + 1;
                return Some(result ^ extra);
            }
            extra ^= 0x80 << (7 * i);
        }
        None
    }

    // Reads a isize varint limited to i32::MAX (used for lengths)
    pub fn read_size(&mut self) -> Option<isize> {
        let mut result = 0;
        let mut extra = 0;
        for i in 0..5 {
            let b = self[i];
            if i == 4 && (b == 0 || b > 7) {
                return None;
            }
            result ^= (b as isize) << (7 * i);
            if b < 0x80 {
                *self += i + 1;
                return Some(result ^ extra);
            }
            extra ^= 0x80 << (7 * i);
        }
        None
    }

    pub fn read_unaligned<T>(&mut self) -> T {
        let p = self.0.as_ptr();
        let value = unsafe { std::ptr::read_unaligned(p as *const T) };
        *self += std::mem::size_of::<T>() as isize;
        value
    }

    pub fn read_slice(&mut self, len: isize) -> &[u8] {
        let p = self.0.as_ptr();
        let slice = unsafe { std::slice::from_raw_parts(p, len as usize) };
        *self += len;
        slice
    }
}

impl PartialEq<NonNull<u8>> for ReadCursor {
    fn eq(&self, other: &NonNull<u8>) -> bool {
        self.0.as_ptr() == other.as_ptr()
    }
}

impl PartialOrd<NonNull<u8>> for ReadCursor {
    fn partial_cmp(&self, other: &NonNull<u8>) -> Option<std::cmp::Ordering> {
        Some(self.0.cmp(other))
    }
}

impl Add<isize> for ReadCursor {
    type Output = ReadCursor;
    fn add(self, rhs: isize) -> Self::Output {
        ReadCursor(unsafe { self.0.offset(rhs) })
    }
}

impl AddAssign<isize> for ReadCursor {
    fn add_assign(&mut self, rhs: isize) {
        *self = *self + rhs;
    }
}

impl Sub<NonNull<u8>> for ReadCursor {
    type Output = isize;
    fn sub(self, rhs: NonNull<u8>) -> Self::Output {
        self.0.as_ptr() as isize - rhs.as_ptr() as isize
    }
}

impl Index<isize> for ReadCursor {
    type Output = u8;
    fn index(&self, index: isize) -> &Self::Output {
        unsafe { &*self.0.as_ptr().offset(index) }
    }
}

#[derive(Clone, Copy)]
pub struct WriteCursor(NonNull<u8>);

impl WriteCursor {
    pub fn new(buffer: &mut [u8]) -> (Self, NonNull<u8>) {
        let ptr = WriteCursor(NonNull::from_ref(&mut buffer[0]));
        let end = ptr.limit(buffer.len() as isize);
        (ptr, end)
    }

    pub fn limit(&self, len: isize) -> NonNull<u8> {
        (*self + len).0
    }

    pub fn write_varint(&mut self, mut n: u64) {
        while n >= 0x80 {
            self[0] = n as u8 | 0x80;
            *self += 1;
            n >>= 7;
        }
        self[0] = n as u8;
        *self += 1;
    }

    pub fn write_unaligned<T>(&mut self, value: T) {
        let p = self.0.as_ptr();
        unsafe {
            std::ptr::write_unaligned(p as *mut T, value);
        }
        *self += std::mem::size_of::<T>() as isize;
    }

    pub fn write_slice(&mut self, slice: &[u8]) {
        let len = slice.len();
        let p = self.0.as_ptr();
        unsafe {
            std::ptr::copy_nonoverlapping(slice.as_ptr(), p, len);
        }
        *self += len as isize;
    }
}

impl PartialEq<NonNull<u8>> for WriteCursor {
    fn eq(&self, other: &NonNull<u8>) -> bool {
        self.0.as_ptr() == other.as_ptr()
    }
}

impl PartialOrd<NonNull<u8>> for WriteCursor {
    fn partial_cmp(&self, other: &NonNull<u8>) -> Option<std::cmp::Ordering> {
        Some(self.0.cmp(other))
    }
}

impl Add<isize> for WriteCursor {
    type Output = WriteCursor;
    fn add(self, rhs: isize) -> Self::Output {
        WriteCursor(unsafe { self.0.offset(rhs) })
    }
}

impl AddAssign<isize> for WriteCursor {
    fn add_assign(&mut self, rhs: isize) {
        *self = *self + rhs;
    }
}

impl Sub<NonNull<u8>> for WriteCursor {
    type Output = isize;
    fn sub(self, rhs: NonNull<u8>) -> Self::Output {
        self.0.as_ptr() as isize - rhs.as_ptr() as isize
    }
}

impl Index<isize> for WriteCursor {
    type Output = u8;
    fn index(&self, index: isize) -> &Self::Output {
        unsafe { &*self.0.as_ptr().offset(index) }
    }
}

impl IndexMut<isize> for WriteCursor {
    fn index_mut(&mut self, index: isize) -> &mut Self::Output {
        unsafe { &mut *self.0.as_ptr().offset(index) }
    }
}

