use core::alloc::Layout;

use crate::{
    arena::Arena,
    containers::{Bytes, RepeatedField},
};

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct Message(pub *mut Object);

unsafe impl Send for Message {}
unsafe impl Sync for Message {}

impl Message {
    pub const fn new<T>(msg: &T) -> Self {
        Message(msg as *const T as *mut T as *mut Object)
    }
}

pub struct Object;

impl Object {
    pub fn create(size: u32, arena: &mut Arena) -> &'static mut Object {
        unsafe {
            let buffer = arena
                .alloc_raw(Layout::from_size_align_unchecked(
                    size as usize,
                    core::mem::align_of::<u64>(),
                ))
                .as_ptr();
            core::ptr::write_bytes(buffer, 0, size as usize);
            &mut *(buffer as *mut Object)
        }
    }

    pub(crate) fn ref_at<T>(&self, offset: usize) -> &T {
        unsafe { &*((self as *const Self as *const u8).add(offset) as *const T) }
    }

    pub(crate) fn ref_mut<T>(&mut self, offset: u32) -> &mut T {
        unsafe { &mut *((self as *mut Object as *mut u8).add(offset as usize) as *mut T) }
    }

    pub fn has_bit(&self, has_bit_idx: u8) -> bool {
        debug_assert!(has_bit_idx < 64);
        (*self.ref_at::<u64>(0)) & (1 << has_bit_idx) != 0
    }

    pub fn set_has_bit(&mut self, has_bit_idx: u32) {
        debug_assert!(has_bit_idx < 64);
        *self.ref_mut::<u64>(0) |= 1 << has_bit_idx;
    }

    pub(crate) fn get<T: Copy>(&self, offset: usize) -> T {
        *self.ref_at::<T>(offset)
    }

    pub(crate) fn get_slice<T>(&self, offset: usize) -> &[T] {
        self.ref_at::<RepeatedField<T>>(offset).as_ref()
    }

    pub(crate) fn set<T>(&mut self, offset: u32, has_bit_idx: u32, val: T) -> &mut T {
        self.set_has_bit(has_bit_idx);
        let field = self.ref_mut::<T>(offset);
        *field = val;
        field
    }

    pub(crate) fn add<T>(&mut self, offset: u32, val: T, arena: &mut Arena) {
        let field = self.ref_mut::<RepeatedField<T>>(offset);
        field.push(val, arena);
    }

    pub(crate) fn bytes(&self, offset: usize) -> &[u8] {
        self.ref_at::<Bytes>(offset).as_ref()
    }

    pub(crate) fn set_bytes(
        &mut self,
        offset: u32,
        has_bit_idx: u32,
        bytes: &[u8],
        arena: &mut Arena,
    ) -> &mut Bytes {
        self.set_has_bit(has_bit_idx);
        let field = self.ref_mut::<Bytes>(offset);
        field.assign(bytes, arena);
        field
    }

    pub(crate) fn add_bytes(&mut self, offset: u32, bytes: &[u8], arena: &mut Arena) -> &mut Bytes {
        let field = self.ref_mut::<RepeatedField<Bytes>>(offset);
        let b = Bytes::from_slice(bytes, arena);
        field.push(b, arena);
        field.last_mut().unwrap()
    }
}
