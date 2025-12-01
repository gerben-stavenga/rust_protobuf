use crate::repeated_field::{Bytes, RepeatedField};

pub struct Object;

impl Object {
    pub fn create(size: u32) -> &'static mut Object {
        let buffer = vec![0u64; (size as usize).div_ceil(8)].leak();
        unsafe { &mut *(buffer as *mut [u64] as *mut Object) }
    }

    pub(crate) fn ref_at<T>(&self, offset: usize) -> &T {
        unsafe { &*((self as *const Self as *const u8).add(offset) as *const T) }
    }

    pub(crate) fn ref_mut<T>(&mut self, offset: u32) -> &mut T {
        unsafe { &mut *((self as *mut Object as *mut u8).add(offset as usize) as *mut T) }
    }

    pub fn has_bit(&self, has_bit_idx: u8) -> bool {
        let has_bit_idx = has_bit_idx as usize;
        let has_bit_offset = has_bit_idx / 32;
        (*self.ref_at::<u32>(has_bit_offset * 4)) & (1 << (has_bit_idx % 32)) != 0
    }

    pub fn set_has_bit(&mut self, has_bit_idx: u32) {
        let has_bit_offset = has_bit_idx / 32;
        *self.ref_mut::<u32>(has_bit_offset * 4) |= 1 << (has_bit_idx as usize % 32);
    }

    pub(crate) fn get<T: Copy>(&self, offset: usize) -> T {
        *self.ref_at::<T>(offset)
    }

    pub(crate) fn get_slice<T>(&self, offset: usize) -> &[T] {
        self.ref_at::<RepeatedField<T>>(offset as usize).as_ref()
    }

    pub(crate) fn set<T>(&mut self, offset: u32, has_bit_idx: u32, val: T) -> &mut T {
        self.set_has_bit(has_bit_idx);
        let field = self.ref_mut::<T>(offset);
        *field = val;
        field
    }

    pub(crate) fn add<T>(&mut self, offset: u32, val: T) {
        let field = self.ref_mut::<RepeatedField<T>>(offset);
        field.push(val);
    }

    pub(crate) fn bytes(&self, offset: usize) -> &[u8] {
        self.ref_at::<Bytes>(offset as usize).as_ref()
    }

    pub(crate) fn set_bytes(&mut self, offset: u32, has_bit_idx: u32, bytes: &[u8]) -> &mut Bytes {
        self.set_has_bit(has_bit_idx);
        let field = self.ref_mut::<Bytes>(offset);
        field.assign(bytes);
        field
    }

    pub(crate) fn add_bytes(&mut self, offset: u32, bytes: &[u8]) -> &mut Bytes {
        let field = self.ref_mut::<RepeatedField<Bytes>>(offset);
        let b = Bytes::from_slice(bytes);
        field.push(b);
        field.last_mut().unwrap()
    }
}
