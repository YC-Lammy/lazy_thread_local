// Copyright 2023 YC Lam
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Lazy Per-object thread-local storage
//!
//! This library provides the `ThreadLocal` type which allows a separate copy of
//! an object to be used for each thread. This allows for per-object
//! thread-local storage, unlike the crate `thread_local`, this crate provides
//! lazy initialisation and does not depend on std.
//!
//! Per-thread objects are not destroyed when a thread exits. Instead, objects
//! are only destroyed when the `ThreadLocal` containing them is dropped.
//!
//! This crate uses platform dependent methods to create thread local keys.
//! On Unix, pthread local storage is used. On windows, Fibers storage is used.
//! On wasm, it relies on std to provide thread id.
//!
//! # Examples
//!
//! Basic usage of `ThreadLocal`:
//!
//! ```rust
//! use lazy_thread_local::ThreadLocal;
//! let mut tls: ThreadLocal<u32> = ThreadLocal::new(||5);
//! assert_eq!(tls.get(), &5);
//! *tls = 6;
//! assert_eq!(tls.get(), &6);
//! ```
//!
//! Initialising `ThreadLocal` in constant context:
//!
//! ```rust
//! use lazy_thread_local::ThreadLocal;
//!
//! static TLS: ThreadLocal<u32> = ThreadLocal::const_new(5);
//!
//! assert_eq!(TLS.get(), 5);
//! ```
//!

use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_family = "wasm")]
mod wasm32;

pub trait ThreadLocalInitialiser<T>: Sized {
    fn init(&self) -> T;
}

impl<F, T> ThreadLocalInitialiser<T> for F
where
    F: Fn() -> T,
{
    fn init(&self) -> T {
        self()
    }
}

pub trait Allocator {
    fn allocate(size: usize) -> *mut u8;
    fn deallocate(ptr: *mut u8);
}

mod private {
    #[cfg(any(unix, windows))]
    pub type DefaultAllocator = CAllocator;

    #[cfg(any(unix, windows))]
    pub struct CAllocator;

    #[cfg(any(unix, windows))]
    impl super::Allocator for CAllocator {
        fn allocate(size: usize) -> *mut u8 {
            unsafe { libc::malloc(size) as *mut u8 }
        }
        fn deallocate(ptr: *mut u8) {
            unsafe { libc::free(ptr as _) };
        }
    }

    #[cfg(not(any(unix, windows)))]
    pub type DefaultAllocator = RAllocator;

    #[cfg(not(any(unix, windows)))]
    pub struct RAllocator;

    #[cfg(not(any(unix, windows)))]
    impl super::Allocator for RAllocator {
        fn allocate(size: usize) -> *mut u8 {
            let new_size = size + core::mem::size_of::<usize>();
            unsafe {
                let ptr = std::alloc::alloc(std::alloc::Layout::array::<u8>(new_size).unwrap());

                let ptr = ptr as *mut usize;
                ptr.write(new_size);

                return ptr.add(1) as *mut u8;
            }
        }

        fn deallocate(ptr: *mut u8) {
            unsafe {
                let ptr = (ptr as *mut usize).sub(1);
                let len = ptr.read();

                std::alloc::dealloc(ptr as _, std::alloc::Layout::array::<u8>(len).unwrap());
            }
        }
    }
}

#[cfg(target_family = "unix")]
type Key = libc::pthread_key_t;

#[cfg(windows)]
type Key = winapi::shared::minwindef::DWORD;

#[cfg(target_family = "wasm")]
type Key = usize;

pub struct ThreadLocal<T, A: Allocator = private::DefaultAllocator> {
    key_created: AtomicBool,
    key: Key,
    initiatiser: *mut u8,
    initialiser_drop: fn(*mut u8),
    initialiser_init: fn(*mut u8) -> T,
    const_init: Option<T>,
    _mark: PhantomData<A>,
}

#[cfg(target_family = "unix")]
impl<T, A: Allocator> ThreadLocal<T, A> {
    unsafe fn create_key() -> Key {
        unsafe extern "C" fn dtor<T, A: Allocator>(ptr: *mut libc::c_void) {
            if ptr.is_null() {
                return;
            }
            let ptr = ptr as *mut T;
            core::ptr::drop_in_place(ptr);
            A::deallocate(ptr as _);
        }

        let mut key: libc::pthread_key_t = 0;
        let re = libc::pthread_key_create(&mut key, Some(dtor::<T, A>));

        assert_eq!(re, 0);

        return key;
    }

    unsafe fn get_key(key: Key) -> *mut T {
        libc::pthread_getspecific(key) as *mut T
    }

    unsafe fn set_key(key: Key, value: *mut T) {
        libc::pthread_setspecific(key, value as _);
    }

    unsafe fn delete_key(key: Key) {
        libc::pthread_key_delete(key);
    }
}

#[cfg(target_os = "windows")]
impl<T, A: Allocator> ThreadLocal<T, A> {
    unsafe fn create_key() -> Key {
        unsafe extern "system" fn dtor<T, A: Allocator>(ptr: winapi::um::winnt::PVOID) {
            if ptr.is_null() {
                return;
            }
            let ptr = ptr as *mut T;
            core::ptr::drop_in_place(ptr);
            A::deallocate(ptr as _);
        }
        winapi::um::fibersapi::FlsAlloc(Some(dtor::<T, A>))
    }

    unsafe fn get_key(key: Key) -> *mut T {
        winapi::um::fibersapi::FlsGetValue(key) as *mut T
    }

    unsafe fn set_key(key: Key, value: *mut T) {
        winapi::um::fibersapi::FlsSetValue(key, value as _);
    }

    unsafe fn delete_key(key: Key) {
        winapi::um::fibersapi::FlsFree(key);
    }
}

impl<T: Copy, A: Allocator> ThreadLocal<T, A> {
    /// initialise the thread local with a copyable value.
    pub const fn const_new(value: T) -> Self {
        // a placeholder function
        fn dummy_drop(_: *mut u8) {
            // does nothing
        }
        // should never be called
        fn dummy_init<T>(_: *mut u8) -> T {
            unreachable!()
        }

        Self {
            key: 0,
            key_created: AtomicBool::new(false),
            initiatiser: 0 as _,
            initialiser_drop: dummy_drop,
            initialiser_init: dummy_init::<T>,
            const_init: Some(value),
            _mark: PhantomData,
        }
    }
}

impl<T, A: Allocator> ThreadLocal<T, A> {
    pub fn new<I: ThreadLocalInitialiser<T>>(init: I) -> Self {
        // drop function wrapper
        fn initialiser_drop<I: ThreadLocalInitialiser<T>, T, A: Allocator>(ptr: *mut u8) {
            if ptr.is_null() {
                return;
            }
            let ptr = ptr as *mut I;
            unsafe {
                core::ptr::drop_in_place(ptr);
                A::deallocate(ptr as _);
            };
        }

        // init function wrapper
        fn initialiser_init<I: ThreadLocalInitialiser<T>, T>(ptr: *mut u8) -> T {
            let ptr = ptr as *mut I;
            unsafe { ptr.as_mut().unwrap_unchecked().init() }
        }

        unsafe {
            let key = Self::create_key();

            let ptr = A::allocate(core::mem::size_of::<T>()) as *mut T;
            ptr.write(init.init());

            Self::set_key(key, ptr);

            let init_ptr = A::allocate(core::mem::size_of::<I>()) as *mut I;
            init_ptr.write(init);

            return Self {
                key,
                key_created: AtomicBool::new(true),
                initiatiser: init_ptr as _,
                initialiser_drop: initialiser_drop::<I, T, A>,
                initialiser_init: initialiser_init::<I, T>,
                const_init: None,
                _mark: PhantomData,
            };
        }
    }

    #[allow(invalid_reference_casting)]
    fn check_init(&self) {
        if self.const_init.is_some() {
            if !self.key_created.swap(true, Ordering::SeqCst) {
                unsafe {
                    let key = Self::create_key();
                    *(&self.key as *const Key as *mut Key) = key;
                }
            }
        }
    }

    unsafe fn init_value(&self) -> &mut T {
        let ptr = A::allocate(core::mem::size_of::<T>()) as *mut T;

        if let Some(v) = &self.const_init {
            // it is guarantined T is copy
            ptr.write(core::ptr::read(v));
        } else {
            ptr.write((self.initialiser_init)(self.initiatiser));
        }

        Self::set_key(self.key, ptr as _);

        return ptr.as_mut().unwrap_unchecked();
    }

    pub fn get(&self) -> &T {
        self.check_init();

        unsafe {
            let ptr = Self::get_key(self.key);

            if ptr.is_null() {
                return self.init_value();
            };

            return (ptr as *mut T).as_ref().unwrap_unchecked();
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.check_init();

        unsafe {
            let ptr = Self::get_key(self.key);

            if ptr.is_null() {
                return self.init_value();
            };

            return (ptr as *mut T).as_mut().unwrap_unchecked();
        }
    }
}

impl<T, A: Allocator> Drop for ThreadLocal<T, A> {
    fn drop(&mut self) {
        (self.initialiser_drop)(self.initiatiser);

        unsafe {
            Self::delete_key(self.key);
        }
    }
}

unsafe impl<T> Sync for ThreadLocal<T> {}
unsafe impl<T> Send for ThreadLocal<T> {}

impl<T> AsRef<T> for ThreadLocal<T> {
    fn as_ref(&self) -> &T {
        self.get()
    }
}

impl<T> AsMut<T> for ThreadLocal<T> {
    fn as_mut(&mut self) -> &mut T {
        self.get_mut()
    }
}

impl<T> core::ops::Deref for ThreadLocal<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T> core::ops::DerefMut for ThreadLocal<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

impl<T: Default> Default for ThreadLocal<T> {
    fn default() -> Self {
        ThreadLocal::new(T::default)
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for ThreadLocal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return self.get().fmt(f);
    }
}

#[test]
fn t() {
    let mut r: ThreadLocal<u8> = ThreadLocal::new(|| 6);

    assert!(*r == 6);
    *r = 7;
    assert!(*r == 7);
    *r = 8;
    assert!(*r == 8);

    let mut r: ThreadLocal<u8> = ThreadLocal::const_new(6);

    assert!(*r == 6);
    *r = 7;
    assert!(*r == 7);
    *r = 8;
    assert!(*r == 8);
}
