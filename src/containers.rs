use std::alloc::{self, Layout};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr::{self, NonNull};

#[derive(Copy, Clone)]
pub(super) struct RawVec {
    ptr: *mut u8,
    cap: usize,
}

impl RawVec {
    fn new() -> Self {
        // `NonNull::dangling()` doubles as "unallocated" and "zero-sized allocation"
        RawVec {
            ptr: std::ptr::null_mut(),
            cap: 0,
        }
    }

    fn new_zst() -> Self {
        // `NonNull::dangling()` doubles as "unallocated" and "zero-sized allocation"
        RawVec {
            ptr: NonNull::dangling().as_ptr(),
            cap: usize::MAX,
        }
    }

    #[inline(never)]
    fn grow(mut self, new_cap: usize, layout: Layout) -> Self {
        // since we set the capacity to usize::MAX when T has size 0,
        // getting to here necessarily means the Vec is overfull.
        assert!(layout.size() != 0, "capacity overflow");

        let (new_cap, new_layout) = if self.cap == 0 {
            if new_cap == 0 {
                (1, layout)
            } else {
                (new_cap, layout)
            }
        } else {
            // This can't overflow because we ensure self.cap <= isize::MAX.
            let new_cap = if new_cap == 0 {
                2 * self.cap
            } else {
                assert!(new_cap > self.cap);
                new_cap
            };

            let new_layout =
                Layout::from_size_align(layout.size() * new_cap, layout.align()).unwrap();

            (new_cap, new_layout)
        };

        // Ensure that the new allocation doesn't exceed `isize::MAX` bytes.
        assert!(
            new_layout.size() <= isize::MAX as usize,
            "Allocation too large"
        );

        let new_ptr = if self.cap == 0 {
            unsafe { alloc::alloc(new_layout) }
        } else {
            let old_layout =
                Layout::from_size_align(layout.size() * self.cap, layout.align()).unwrap();
            let old_ptr = self.ptr;
            unsafe { alloc::realloc(old_ptr, old_layout, new_layout.size()) }
        };

        // If allocation fails, `new_ptr` will be null, in which case we abort.
        if new_ptr.is_null() {
            alloc::handle_alloc_error(new_layout);
        }
        self.ptr = new_ptr;
        self.cap = new_cap;
        self
    }

    pub unsafe fn push_uninitialized(&mut self, len: &mut usize, layout: Layout) -> *mut u8 {
        let l = *len;
        if l == self.cap {
            *self = self.grow(0, layout);
        }

        // Can't overflow, we'll OOM first.
        *len = l + 1;

        unsafe { self.ptr.add(l * layout.size()) }
    }

    pub unsafe fn pop(&mut self, len: &mut usize, layout: Layout) -> Option<*mut u8> {
        let l = *len;
        if l == 0 {
            None
        } else {
            let l = l - 1;
            let ptr = unsafe { self.ptr.add(l * layout.size()) };
            *len = l;
            Some(ptr)
        }
    }

    pub fn reserve(&mut self, new_cap: usize, layout: Layout) {
        if new_cap > self.cap {
            *self = self.grow(new_cap, layout);
        }
    }

    #[inline(always)]
    fn drop(self, layout: Layout) {
        if self.cap != 0 && layout.size() != 0 {
            unsafe {
                let layout =
                    Layout::from_size_align_unchecked(layout.size() * self.cap, layout.align());
                alloc::dealloc(self.ptr, layout);
            }
        }
    }
}

pub struct RepeatedField<T> {
    buf: RawVec,
    len: usize,
    phantom: std::marker::PhantomData<T>,
}

impl<T> Default for RepeatedField<T> {
    fn default() -> Self {
        RepeatedField {
            buf: if std::mem::size_of::<T>() == 0 {
                RawVec::new_zst()
            } else {
                RawVec::new()
            },
            len: 0,
            phantom: std::marker::PhantomData,
        }
    }
}

impl<T> Debug for RepeatedField<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl<T> RepeatedField<T> {
    fn ptr(&self) -> *mut T {
        self.buf.ptr as *mut T
    }

    fn cap(&self) -> usize {
        self.buf.cap
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_slice(slice: &[T]) -> Self
    where
        T: Copy,
    {
        let mut rf = Self::new();
        rf.append(slice);
        rf
    }

    pub fn slice(&self) -> &[T] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr(), self.len) }
        }
    }

    pub fn slice_mut(&mut self) -> &mut [T] {
        if self.len == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(self.ptr(), self.len) }
        }
    }

    pub fn push(&mut self, elem: T) {
        unsafe {
            (self
                .buf
                .push_uninitialized(&mut self.len, Layout::new::<T>()) as *mut T)
                .write(elem)
        };
    }

    pub fn pop(&mut self) -> Option<T> {
        unsafe {
            self.buf
                .pop(&mut self.len, Layout::new::<T>())
                .map(|ptr| ptr.cast::<T>().read())
        }
    }

    pub fn insert(&mut self, index: usize, elem: T) {
        assert!(index <= self.len, "index out of bounds");
        let len = self.len;
        if len == self.cap() {
            self.buf = self.buf.grow(0, Layout::new::<T>());
        }

        unsafe {
            ptr::copy(
                self.ptr().add(index),
                self.ptr().add(index + 1),
                len - index,
            );
            ptr::write(self.ptr().add(index), elem);
        }

        self.len = len + 1;
    }

    pub fn remove(&mut self, index: usize) -> T {
        let len = self.len;
        assert!(index < len, "index out of bounds");

        let len = len - 1;

        unsafe {
            let result = ptr::read(self.ptr().add(index));
            ptr::copy(
                self.ptr().add(index + 1),
                self.ptr().add(index),
                len - index,
            );
            self.len = len;
            result
        }
    }

    pub fn drain(&mut self) -> Drain<'_, T> {
        let iter = unsafe { RawValIter::new(self) };

        // this is a mem::forget safety thing. If Drain is forgotten, we just
        // leak the whole Vec's contents. Also we need to do this *eventually*
        // anyway, so why not do it now?
        self.len = 0;

        Drain {
            iter,
            vec: PhantomData,
        }
    }

    pub fn clear(&mut self) {
        unsafe { std::ptr::drop_in_place(&mut *self) }
        self.len = 0
    }

    pub fn reserve(&mut self, new_cap: usize) {
        self.buf.reserve(new_cap, Layout::new::<T>());
    }

    pub fn assign(&mut self, slice: &[T])
    where
        T: Copy,
    {
        self.clear();
        self.append(slice);
    }

    pub fn append(&mut self, slice: &[T])
    where
        T: Copy,
    {
        let old_len = self.len;
        self.reserve(old_len + slice.len());
        unsafe {
            self.ptr()
                .add(old_len)
                .copy_from_nonoverlapping(slice.as_ptr(), slice.len());
        }
        self.len = old_len + slice.len();
    }
}

impl<T> Drop for RepeatedField<T> {
    fn drop(&mut self) {
        unsafe {
            std::ptr::drop_in_place(self.deref_mut());
        }
        self.buf.drop(Layout::new::<T>());
    }
}

impl<T> Deref for RepeatedField<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.slice()
    }
}

impl<T> DerefMut for RepeatedField<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.slice_mut()
    }
}

impl<T> IntoIterator for RepeatedField<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;
    fn into_iter(self) -> IntoIter<T> {
        let (iter, buf) = unsafe { (RawValIter::new(&self), ptr::read(&self.buf)) };

        mem::forget(self);

        IntoIter { iter, buf }
    }
}

struct RawValIter<T> {
    start: *const T,
    end: *const T,
}

impl<T> RawValIter<T> {
    unsafe fn new(slice: &[T]) -> Self {
        RawValIter {
            start: slice.as_ptr(),
            end: if mem::size_of::<T>() == 0 {
                ((slice.as_ptr() as usize) + slice.len()) as *const _
            } else if slice.is_empty() {
                slice.as_ptr()
            } else {
                unsafe { slice.as_ptr().add(slice.len()) }
            },
        }
    }
}

impl<T> Iterator for RawValIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                if mem::size_of::<T>() == 0 {
                    self.start = (self.start as usize + 1) as *const _;
                    Some(ptr::read(NonNull::<T>::dangling().as_ptr()))
                } else {
                    let old_ptr = self.start;
                    self.start = self.start.offset(1);
                    Some(ptr::read(old_ptr))
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let elem_size = mem::size_of::<T>();
        let len =
            (self.end as usize - self.start as usize) / if elem_size == 0 { 1 } else { elem_size };
        (len, Some(len))
    }
}

impl<T> DoubleEndedIterator for RawValIter<T> {
    fn next_back(&mut self) -> Option<T> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                if mem::size_of::<T>() == 0 {
                    self.end = (self.end as usize - 1) as *const _;
                    Some(ptr::read(NonNull::<T>::dangling().as_ptr()))
                } else {
                    self.end = self.end.offset(-1);
                    Some(ptr::read(self.end))
                }
            }
        }
    }
}

pub struct IntoIter<T> {
    buf: RawVec,
    iter: RawValIter<T>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T> Drop for IntoIter<T> {
    fn drop(&mut self) {
        for _ in &mut *self {}
        self.buf.drop(Layout::new::<T>())
    }
}

pub struct Drain<'a, T: 'a> {
    vec: PhantomData<&'a mut RepeatedField<T>>,
    iter: RawValIter<T>,
}

impl<'a, T> Iterator for Drain<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T> DoubleEndedIterator for Drain<'a, T> {
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<'a, T> Drop for Drain<'a, T> {
    fn drop(&mut self) {
        // pre-drain the iter
        for _ in &mut *self {}
    }
}

pub type Bytes = RepeatedField<u8>;
