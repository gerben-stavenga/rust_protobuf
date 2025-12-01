use std::{
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

#[repr(C)]
pub(crate) struct Stack<T> {
    sp: usize,
    entries: [MaybeUninit<T>],
}

impl<T> Stack<T> {
    pub(crate) fn push(&mut self, entry: T) -> Option<&mut T> {
        if self.sp == 0 {
            return None;
        }
        self.sp -= 1;
        let slot = &mut self.entries[self.sp];
        Some(slot.write(entry))
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        let sp = self.sp;
        if sp == self.entries.len() {
            return None;
        }
        self.sp = sp + 1;
        Some(unsafe { self.entries[sp].assume_init_read() })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sp == self.entries.len()
    }
}

#[repr(C)]
pub(crate) struct StackWithStorage<T, const N: usize> {
    sp: usize,
    entries: [MaybeUninit<T>; N],
}

impl<T, const N: usize> Default for StackWithStorage<T, N> {
    fn default() -> Self {
        Self {
            sp: N,
            entries: [const { MaybeUninit::uninit() }; N],
        }
    }
}

impl<T, const N: usize> Deref for StackWithStorage<T, N> {
    type Target = Stack<T>;

    fn deref(&self) -> &Self::Target {
        unsafe {
            // convert StackWithStorage<T, N> thin ptr to Stack<T> fat ptr
            let fat_ptr = std::ptr::slice_from_raw_parts(self, N) as *const Stack<T>;
            &*fat_ptr
        }
    }
}

impl<T, const N: usize> DerefMut for StackWithStorage<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // convert StackWithStorage<T, N> thin ptr to Stack<T> fat ptr
            let fat_ptr = std::ptr::slice_from_raw_parts_mut(self, N) as *mut Stack<T>;
            &mut *fat_ptr
        }
    }
}

pub struct LocalCapture<'a, T> {
    value: std::mem::ManuallyDrop<T>,
    origin: &'a mut T,
}

impl<'a, T> LocalCapture<'a, T> {
    pub fn new(origin: &'a mut T) -> Self {
        Self {
            value: std::mem::ManuallyDrop::new(unsafe { std::ptr::read(origin) }),
            origin,
        }
    }
}

impl<'a, T> std::ops::Deref for LocalCapture<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'a, T> std::ops::DerefMut for LocalCapture<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<'a, T> Drop for LocalCapture<'a, T> {
    fn drop(&mut self) {
        unsafe {
            std::ptr::write(self.origin, std::mem::ManuallyDrop::take(&mut self.value));
        }
    }
}
