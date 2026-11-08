//! Type-erased growable column of equally sized items.

use std::alloc::{self, Layout};
use std::ptr::{self, NonNull};

pub(crate) struct BlobVec {
    item: Layout,
    drop: Option<unsafe fn(*mut u8)>,
    data: NonNull<u8>,
    len: usize,
    cap: usize,
}

// SAFETY: BlobVec only ever stores `Component`s, which are `Send + Sync`.
unsafe impl Send for BlobVec {}
unsafe impl Sync for BlobVec {}

impl BlobVec {
    pub fn new(item: Layout, drop: Option<unsafe fn(*mut u8)>) -> Self {
        let data = NonNull::new(item.align() as *mut u8).expect("align is non-zero");
        let cap = if item.size() == 0 { usize::MAX } else { 0 };
        BlobVec { item, drop, data, len: 0, cap }
    }

    fn reserve_one(&mut self) {
        if self.len < self.cap {
            return;
        }
        let new_cap = (self.cap * 2).max(4);
        let new_layout =
            Layout::from_size_align(self.item.size() * new_cap, self.item.align()).expect("column too large");
        let new_ptr = unsafe {
            if self.cap == 0 {
                alloc::alloc(new_layout)
            } else {
                let old = Layout::from_size_align_unchecked(self.item.size() * self.cap, self.item.align());
                alloc::realloc(self.data.as_ptr(), old, new_layout.size())
            }
        };
        self.data = NonNull::new(new_ptr).unwrap_or_else(|| alloc::handle_alloc_error(new_layout));
        self.cap = new_cap;
    }

    /// Pointer to item `i`. Valid for `i <= len` (one-past-end for writing).
    #[inline]
    pub fn ptr_at(&self, i: usize) -> *mut u8 {
        unsafe { self.data.as_ptr().add(i * self.item.size()) }
    }

    pub fn base_ptr(&self) -> *mut u8 {
        self.data.as_ptr()
    }

    /// Moves the item at `src` (bitwise) onto the end. Caller must not drop `src` afterwards.
    pub unsafe fn push(&mut self, src: *const u8) {
        self.reserve_one();
        unsafe { ptr::copy_nonoverlapping(src, self.ptr_at(self.len), self.item.size()) };
        self.len += 1;
    }

    /// Drops item `i` and replaces it with the bytes at `src`.
    pub unsafe fn replace(&mut self, i: usize, src: *const u8) {
        debug_assert!(i < self.len);
        let p = self.ptr_at(i);
        unsafe {
            if let Some(d) = self.drop {
                d(p);
            }
            ptr::copy_nonoverlapping(src, p, self.item.size());
        }
    }

    /// Removes item `i` without dropping it (ownership was taken elsewhere), filling the hole with the last item.
    pub unsafe fn swap_remove_forget(&mut self, i: usize) {
        debug_assert!(i < self.len);
        let last = self.len - 1;
        if i != last {
            unsafe { ptr::copy_nonoverlapping(self.ptr_at(last), self.ptr_at(i), self.item.size()) };
        }
        self.len -= 1;
    }

    pub unsafe fn swap_remove_drop(&mut self, i: usize) {
        debug_assert!(i < self.len);
        unsafe {
            if let Some(d) = self.drop {
                d(self.ptr_at(i));
            }
            self.swap_remove_forget(i);
        }
    }

    /// Moves item `i` to the end of `dst` (same item layout), filling the hole with the last item.
    pub unsafe fn swap_remove_into(&mut self, i: usize, dst: &mut BlobVec) {
        debug_assert_eq!(self.item, dst.item);
        unsafe {
            dst.push(self.ptr_at(i));
            self.swap_remove_forget(i);
        }
    }
}

impl Drop for BlobVec {
    fn drop(&mut self) {
        if let Some(d) = self.drop {
            for i in 0..self.len {
                unsafe { d(self.ptr_at(i)) };
            }
        }
        if self.item.size() != 0 && self.cap != 0 {
            unsafe {
                alloc::dealloc(
                    self.data.as_ptr(),
                    Layout::from_size_align_unchecked(self.item.size() * self.cap, self.item.align()),
                )
            };
        }
    }
}
