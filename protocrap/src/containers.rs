use core::alloc::Layout;
use core::fmt::Debug;
use core::marker::PhantomData;
use core::mem;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};

#[derive(Copy, Clone)]
pub(super) struct RawVec {
    ptr: *mut u8,
    cap: usize,
}

unsafe impl Send for RawVec {}
unsafe impl Sync for RawVec {}

impl RawVec {
    const fn new() -> Self {
        // `NonNull::dangling()` doubles as "unallocated" and "zero-sized allocation"
        RawVec {
            ptr: core::ptr::null_mut(),
            cap: 0,
        }
    }

    #[inline(never)]
    fn grow(mut self, new_cap: usize, layout: Layout, arena: &mut crate::arena::Arena) -> Self {
        // since we set the capacity to usize::MAX when T has size 0,
        // getting to here necessarily means the Vec is overfull.
        assert!(layout.size() != 0, "capacity overflow");

        let (new_cap, new_layout) = if self.cap == 0 {
            if new_cap == 0 {
                (1, layout)
            } else {
                let new_layout =
                    Layout::from_size_align(layout.size() * new_cap, layout.align()).unwrap();
                (new_cap, new_layout)
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
            arena.alloc_raw(new_layout).as_ptr()
        } else {
            let new_ptr = arena.alloc_raw(new_layout).as_ptr();
            unsafe { core::ptr::copy_nonoverlapping(self.ptr, new_ptr, layout.size() * self.cap) };
            new_ptr
        };

        // If allocation fails, `new_ptr` will be null, in which case we abort.
        if new_ptr.is_null() {
            // TODO: use a better error handling strategy
            panic!("allocation failed");
        }
        self.ptr = new_ptr;
        self.cap = new_cap;
        self
    }

    pub unsafe fn push_uninitialized(
        &mut self,
        len: &mut usize,
        layout: Layout,
        arena: &mut crate::arena::Arena,
    ) -> *mut u8 {
        let l = *len;
        if l == self.cap {
            *self = self.grow(0, layout, arena);
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

    pub fn reserve(&mut self, new_cap: usize, layout: Layout, arena: &mut crate::arena::Arena) {
        if new_cap > self.cap {
            *self = self.grow(new_cap, layout, arena);
        }
    }
}

pub struct RepeatedField<T> {
    buf: RawVec,
    len: usize,
    phantom: core::marker::PhantomData<T>,
}

impl<T: PartialEq> PartialEq for RepeatedField<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: PartialEq> PartialEq<&[T]> for RepeatedField<T> {
    fn eq(&self, other: &&[T]) -> bool {
        self.as_ref() == *other
    }
}

impl<T: Eq> Eq for RepeatedField<T> where T: Eq {}

impl<T> Default for RepeatedField<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Debug for RepeatedField<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl<T> RepeatedField<T> {
    const fn ptr(&self) -> *mut T {
        self.buf.ptr as *mut T
    }

    const fn cap(&self) -> usize {
        self.buf.cap
    }

    pub const fn new() -> Self {
        RepeatedField {
            buf: RawVec::new(),
            len: 0,
            phantom: core::marker::PhantomData,
        }
    }

    pub fn from_slice(slice: &[T], arena: &mut crate::arena::Arena) -> Self
    where
        T: Copy,
    {
        let mut rf = Self::new();
        rf.append(slice, arena);
        rf
    }

    pub const fn from_static(slice: &'static [T]) -> Self {
        RepeatedField {
            buf: RawVec {
                ptr: slice.as_ptr() as *mut u8,
                cap: slice.len(),
            },
            len: slice.len(),
            phantom: PhantomData,
        }
    }

    pub const fn slice(&self) -> &[T] {
        if self.cap() == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr(), self.len) }
        }
    }

    pub fn slice_mut(&mut self) -> &mut [T] {
        if self.cap() == 0 {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(self.ptr(), self.len) }
        }
    }

    pub fn push(&mut self, elem: T, arena: &mut crate::arena::Arena) {
        unsafe {
            (self
                .buf
                .push_uninitialized(&mut self.len, Layout::new::<T>(), arena)
                as *mut T)
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

    pub fn insert(&mut self, index: usize, elem: T, arena: &mut crate::arena::Arena) {
        assert!(index <= self.len, "index out of bounds");
        let len = self.len;
        if len == self.cap() {
            self.buf = self.buf.grow(0, Layout::new::<T>(), arena);
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
        unsafe { core::ptr::drop_in_place(self.as_mut()) }
        self.len = 0
    }

    pub fn reserve(&mut self, new_cap: usize, arena: &mut crate::arena::Arena) {
        self.buf.reserve(new_cap, Layout::new::<T>(), arena);
    }

    pub fn assign(&mut self, slice: &[T], arena: &mut crate::arena::Arena)
    where
        T: Copy,
    {
        self.clear();
        self.append(slice, arena);
    }

    pub fn append(&mut self, slice: &[T], arena: &mut crate::arena::Arena)
    where
        T: Copy,
    {
        let old_len = self.len;
        self.reserve(old_len + slice.len(), arena);
        unsafe {
            self.ptr()
                .add(old_len)
                .copy_from_nonoverlapping(slice.as_ptr(), slice.len());
        }
        self.len = old_len + slice.len();
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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct String(Bytes);
impl String {
    pub const fn new() -> Self {
        String(RepeatedField::new())
    }

    pub const fn from_static(s: &'static str) -> Self {
        String(RepeatedField::from_static(s.as_bytes()))
    }

    pub const fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(self.0.slice()) }
    }

    pub fn assign(&mut self, s: &str, arena: &mut crate::arena::Arena) {
        self.0.assign(s.as_bytes(), arena);
    }
}

impl Deref for String {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
