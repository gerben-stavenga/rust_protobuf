use core::{
    ops::{Add, AddAssign, Index, IndexMut, Sub},
    ptr::NonNull,
};

pub(crate) const SLOP_SIZE: usize = 16;

pub fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ (-((n & 1) as i64))
}

pub fn zigzag_encode(n: i64) -> u64 {
    ((n as u64) << 1) ^ ((n >> 63) as u64)
}

#[derive(Clone, Copy)]
pub struct ReadCursor(pub NonNull<u8>);

#[inline(never)]
fn read_varint(ptr: *const u8) -> (*const u8, u64) {
    let mut result = 0;
    let mut extra = 0;
    for i in 0..10 {
        let b = unsafe { *ptr.add(i) };
        if i == 9 && b != 1 {
            break;
        }
        result ^= (b as u64) << (7 * i);
        if b < 0x80 {
            let new_ptr = unsafe { ptr.add(i + 1) };
            return (new_ptr, result ^ extra);
        }
        extra ^= 0x80 << (7 * i);
    }
    (core::ptr::null(), 0)
}

#[inline(never)]
fn read_tag(ptr: *const u8) -> (*const u8, u32) {
    let mut result = 0;
    let mut extra = 0;
    for i in 0..5 {
        let b = unsafe { *ptr.add(i) };
        if i == 4 && (b == 0 || b > 15) {
            break;
        }
        result ^= (b as u32) << (7 * i);
        if b < 0x80 {
            let new_ptr = unsafe { ptr.add(i + 1) };
            return (new_ptr, result ^ extra);
        }
        extra ^= 0x80 << (7 * i);
    }
    (core::ptr::null(), 0)
}

#[inline(never)]
fn read_size(ptr: *const u8) -> (*const u8, isize) {
    let mut result = 0;
    let mut extra = 0;
    for i in 0..5 {
        let b = unsafe { *ptr.add(i) };
        if i == 4 && (b == 0 || b > 7) {
            break;
        }
        result ^= (b as isize) << (7 * i);
        if b < 0x80 {
            let new_ptr = unsafe { ptr.add(i + 1) };
            return (new_ptr, result ^ extra);
        }
        extra ^= 0x80 << (7 * i);
    }
    (core::ptr::null(), 0)
}

impl ReadCursor {
    pub fn new(buffer: &[u8]) -> (Self, NonNull<u8>) {
        let ptr = ReadCursor(NonNull::from_ref(&buffer[0]));
        let end = (ptr + buffer.len() as isize).0;
        (ptr, end)
    }

    #[inline(always)]
    pub fn read_varint(&mut self) -> Option<u64> {
        let res = self[0] as u64;
        if core::hint::likely(res < 0x80) {
            *self += 1;
            Some(res)
        } else {
            let (new_ptr, value) = read_varint(self.0.as_ptr());
            if new_ptr.is_null() {
                return None;
            }
            self.0 = unsafe { NonNull::new_unchecked(new_ptr as *mut u8) };
            Some(value)
        }
    }

    #[inline(always)]
    pub fn read_tag(&mut self) -> Option<u32> {
        let res = self[0] as u32;
        if core::hint::likely(res < 0x80) {
            *self += 1;
            Some(res)
        } else {
            let (new_ptr, value) = read_tag(self.0.as_ptr());
            if new_ptr.is_null() {
                return None;
            }
            self.0 = unsafe { NonNull::new_unchecked(new_ptr as *mut u8) };
            Some(value)
        }
    }

    // Reads a isize varint limited to i32::MAX (used for lengths)
    #[inline(always)]
    pub fn read_size(&mut self) -> Option<isize> {
        let res = self[0] as isize;
        if core::hint::likely(res < 0x80) {
            *self += 1;
            Some(res)
        } else {
            let (new_ptr, value) = read_size(self.0.as_ptr());
            if new_ptr.is_null() {
                return None;
            }
            self.0 = unsafe { NonNull::new_unchecked(new_ptr as *mut u8) };
            Some(value)
        }
    }

    #[inline(always)]
    pub fn read_unaligned<T>(&mut self) -> T {
        let p = self.0.as_ptr();
        let value = unsafe { core::ptr::read_unaligned(p as *const T) };
        *self += core::mem::size_of::<T>() as isize;
        value
    }

    #[inline(always)]
    pub fn read_slice(&mut self, len: isize) -> &[u8] {
        let p = self.0.as_ptr();
        let slice = unsafe { core::slice::from_raw_parts(p, len as usize) };
        *self += len;
        slice
    }

    #[inline(always)]
    pub fn peek_tag(&self) -> u32 {
        unsafe { core::ptr::read_unaligned(self.0.as_ptr() as *const u16) as u32 }
    }
}

impl PartialEq<NonNull<u8>> for ReadCursor {
    #[inline(always)]
    fn eq(&self, other: &NonNull<u8>) -> bool {
        self.0.as_ptr() == other.as_ptr()
    }
}

impl PartialOrd<NonNull<u8>> for ReadCursor {
    fn partial_cmp(&self, other: &NonNull<u8>) -> Option<core::cmp::Ordering> {
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
    #[inline(always)]
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
    #[inline(always)]
    fn index(&self, index: isize) -> &Self::Output {
        unsafe { &*self.0.as_ptr().offset(index) }
    }
}

fn varint_size(n: u64) -> isize {
    let log2 = (n | 1).ilog2();
    ((log2 * 9 + 64 + 9) / 64) as isize
}

#[derive(Clone, Copy)]
pub struct WriteCursor(pub NonNull<u8>);

impl WriteCursor {
    pub fn new(buffer: &mut [u8]) -> (Self, NonNull<u8>) {
        let mut ptr = WriteCursor(NonNull::from_ref(&buffer[0]));
        let end = ptr.0;
        ptr += buffer.len() as isize;
        (ptr, end)
    }

    pub fn write_varint(&mut self, mut n: u64) {
        *self += -varint_size(n);
        let mut i = 0;
        while n >= 0x80 {
            self[i] = n as u8 | 0x80;
            n >>= 7;
            i += 1;
        }
        self[i] = n as u8;
    }

    pub fn write_unaligned<T>(&mut self, value: T) {
        *self += -(core::mem::size_of::<T>() as isize);
        let p = self.0.as_ptr();
        unsafe {
            core::ptr::write_unaligned(p as *mut T, value);
        }
    }

    pub fn write_slice(&mut self, slice: &[u8]) {
        let len = slice.len();
        *self += -(len as isize);
        let p = self.0.as_ptr();
        unsafe {
            core::ptr::copy_nonoverlapping(slice.as_ptr(), p, len);
        }
    }

    pub fn write_tag(&mut self, tag: u32) {
        // TODO optimize
        self.write_varint(tag as u64);
    }
}

impl PartialEq<NonNull<u8>> for WriteCursor {
    fn eq(&self, other: &NonNull<u8>) -> bool {
        self.0.as_ptr() == other.as_ptr()
    }
}

impl PartialOrd<NonNull<u8>> for WriteCursor {
    fn partial_cmp(&self, other: &NonNull<u8>) -> Option<core::cmp::Ordering> {
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

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldKind {
    Unknown,
    Varint64,
    Varint32,
    Varint64Zigzag,
    Varint32Zigzag,
    Bool,
    Fixed64,
    Fixed32,
    Bytes,
    Message,
    Group,
    RepeatedVarint64,
    RepeatedVarint32,
    RepeatedVarint64Zigzag,
    RepeatedVarint32Zigzag,
    RepeatedBool,
    RepeatedFixed64,
    RepeatedFixed32,
    RepeatedBytes,
    RepeatedMessage,
    RepeatedGroup,
}
