use std::alloc::{Allocator, Layout};
use std::ptr;
use std::ptr::NonNull;

// Arena allocates memory for protobuf objects. Which can be freed all at once.
// This is useful for short lived objects that are created and destroyed together.
// We need arena to be a non-generic type to avoid code bloat, but at the same time
// we want users to have full control over the allocator used by the arena. Because
// arena is batching small allocations into sporadic large allocations, we can
// allocate large blocks using the dyn Allocator trait object without too much
// overhead.
pub struct Arena<'a> {
    current: *mut MemBlock,
    cursor: *mut u8,
    end: *mut u8,
    allocator: &'a dyn std::alloc::Allocator,
}

// Mem block is a block of contiguous memory allocated from the allocator
struct MemBlock {
    prev: *mut MemBlock,
    layout: Layout, // Layout of the entire block including header
}

const DEFAULT_BLOCK_SIZE: usize = 8 * 1024; // 8KB initial block
const MAX_BLOCK_SIZE: usize = 1024 * 1024; // 1MB max block

impl<'a> Arena<'a> {
    /// Create a new arena with the given allocator
    pub fn new(allocator: &'a dyn Allocator) -> Self {
        Self {
            current: ptr::null_mut(),
            cursor: ptr::null_mut(),
            end: ptr::null_mut(),
            allocator,
        }
    }

    /// Allocate uninitialized memory for type T, returning a raw pointer
    pub fn alloc<T>(&mut self) -> *mut T {
        let layout = Layout::new::<T>();
        let ptr = self.alloc_raw(layout);
        ptr.as_ptr() as *mut T
    }

    /// Allocate an uninitialized slice of T with given length
    pub fn alloc_slice<T>(&mut self, len: usize) -> *mut [T] {
        let layout = Layout::array::<T>(len).expect("Layout overflow");
        let ptr = self.alloc_raw(layout);

        ptr::slice_from_raw_parts_mut(ptr.as_ptr() as *mut T, len)
    }

    /// Allocate raw memory with given size and alignment (uninitialized)
    #[inline]
    pub fn alloc_raw(&mut self, layout: Layout) -> NonNull<u8> {
        let size = layout.size();
        let align = layout.align();

        // Align the cursor to the required alignment
        let cursor_addr = self.cursor as usize;
        let aligned_addr = (cursor_addr + align - 1) & !(align - 1);
        let aligned_cursor = aligned_addr as *mut u8;

        // Check if we have enough space: end - aligned_cursor >= size
        let available = self.end as usize - aligned_cursor as usize;
        if std::hint::likely(available >= size) {
            // Fits in current block - use it regardless of size
            self.cursor = unsafe { aligned_cursor.add(size) };
            return unsafe { NonNull::new_unchecked(aligned_cursor) };
        }

        // Doesn't fit - need new allocation strategy
        self.alloc_outlined(layout, available)
    }

    /// Get total bytes allocated by this arena
    pub fn bytes_allocated(&self) -> usize {
        let mut total = 0;
        let mut current = self.current;

        unsafe {
            while !current.is_null() {
                total += (*current).layout.size();
                current = (*current).prev;
            }
        }

        total
    }

    /// Allocate a new memory block - never inlined to keep fast path small
    #[inline(never)]
    fn alloc_outlined(&mut self, layout: Layout, available: usize) -> NonNull<u8> {
        const SIGNIFICANT_SPACE_THRESHOLD: usize = 512; // 512 bytes is "significant"

        if available >= SIGNIFICANT_SPACE_THRESHOLD {
            // Significant free space left, which implies this is a large allocation
            // Keep the free space and just allocate a dedicated block for this allocation
            // and keep the current block for future allocations.
            self.alloc_dedicated(layout)
        } else {
            // Little space left - allocate new block sized for this allocation + future allocations
            self.allocate_new_block(layout)
        }
    }

    /// Allocate a new memory block
    fn allocate_new_block(&mut self, alloc_layout: Layout) -> NonNull<u8> {
        // Calculate block size - grow exponentially but respect min_size

        let (layout, offset) = Layout::new::<MemBlock>()
            .extend(alloc_layout)
            .expect("Layout overflow");
        let layout = layout.pad_to_align();

        let new_block_size = if self.current.is_null() {
            DEFAULT_BLOCK_SIZE
        } else {
            let current_block_size = unsafe { (*self.current).layout.size() };
            (current_block_size * 2).min(MAX_BLOCK_SIZE)
        };

        let (layout, block_start) = layout
            .extend(Layout::array::<u8>(new_block_size).expect("Layout overflow"))
            .expect("Layout overflow");
        let layout = layout.pad_to_align();

        let ptr = self
            .allocator
            .allocate(layout)
            .expect("Allocation failed")
            .as_ptr() as *mut MemBlock;

        unsafe {
            // Initialize the MemBlock header
            (*ptr).prev = self.current;
            (*ptr).layout = layout;

            // Update arena state - this becomes the new active block
            self.current = ptr;
            self.cursor = (ptr as *mut u8).add(block_start);
            self.end = (ptr as *mut u8).add(layout.size());
            NonNull::new_unchecked((ptr as *mut u8).add(offset))
        }
    }

    /// Allocate a dedicated (large) memory directly from allocator (dedicated block)
    fn alloc_dedicated(&mut self, layout: Layout) -> NonNull<u8> {
        // Use layout extend for proper alignment
        let memblock_layout = Layout::new::<MemBlock>();
        let (extended_layout, data_offset) =
            memblock_layout.extend(layout).expect("Layout overflow");
        let final_layout = extended_layout.pad_to_align();

        let ptr = self
            .allocator
            .allocate(final_layout)
            .expect("Allocation failed")
            .as_ptr() as *mut MemBlock;

        unsafe {
            (*ptr).layout = final_layout;

            // Insert just after current head, keeping current as head
            if !self.current.is_null() {
                // Insert between current and current.prev
                (*ptr).prev = (*self.current).prev;
                (*self.current).prev = ptr;
            } else {
                // No blocks yet, this becomes the only block
                (*ptr).prev = ptr::null_mut();
                self.current = ptr;
                // Still no active bump allocation (cursor/end remain null)
            }

            // Return aligned data pointer after header
            let data_ptr = (ptr as *mut u8).add(data_offset);
            NonNull::new_unchecked(data_ptr)
        }
    }
}

impl<'a> Drop for Arena<'a> {
    fn drop(&mut self) {
        unsafe {
            let mut current = self.current;
            while !current.is_null() {
                let prev = (*current).prev;
                let layout = (*current).layout;

                // Deallocate this block with correct size
                let ptr = NonNull::new_unchecked(current as *mut u8);
                self.allocator.deallocate(ptr, layout);

                current = prev;
            }
        }
    }
}

// Safety: Arena can be sent between threads if the allocator supports it
unsafe impl<'a> Send for Arena<'a> where &'a dyn Allocator: Send {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::Global;

    #[test]
    fn test_basic_allocation() {
        let mut arena = Arena::new(&Global);

        let ptr1: *mut u32 = arena.alloc();
        let ptr2: *mut u64 = arena.alloc();

        unsafe {
            *ptr1 = 42;
            *ptr2 = 1337;

            assert_eq!(*ptr1, 42);
            assert_eq!(*ptr2, 1337);
        }
    }

    #[test]
    fn test_slice_allocation() {
        let mut arena = Arena::new(&Global);

        let slice_ptr: *mut [u32] = arena.alloc_slice(100);

        unsafe {
            let slice = &mut *slice_ptr;
            slice[0] = 1;
            slice[99] = 2;

            assert_eq!(slice.len(), 100);
            assert_eq!(slice[0], 1);
            assert_eq!(slice[99], 2);
        }
    }

    #[test]
    fn test_alignment() {
        let mut arena = Arena::new(&Global);

        // Allocate types with different alignment requirements
        let _u8_ptr: *mut u8 = arena.alloc();
        let u64_ptr: *mut u64 = arena.alloc();

        // Check that u64 is properly aligned
        assert_eq!(u64_ptr as usize % std::mem::align_of::<u64>(), 0);
    }

    #[test]
    fn test_large_allocation() {
        let mut arena = Arena::new(&Global);

        // Allocate something larger than default block size
        let large_slice_ptr: *mut [u8] = arena.alloc_slice(DEFAULT_BLOCK_SIZE * 2);

        unsafe {
            let large_slice = &mut *large_slice_ptr;
            large_slice[0] = 1;
            large_slice[large_slice.len() - 1] = 2;

            assert_eq!(large_slice[0], 1);
            assert_eq!(large_slice[large_slice.len() - 1], 2);
        }
    }
}
