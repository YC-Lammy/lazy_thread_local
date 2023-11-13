use core::sync::atomic::{AtomicUsize, Ordering};

use crate::Allocator;
use crate::ThreadLocal;

struct KeyStore {
    thread_id: u64,
    key: usize,
    value: usize,
    dtor: Option<unsafe extern "C" fn(*mut u8)>,
}

impl PartialEq for KeyStore {
    fn eq(&self, other: &Self) -> bool {
        self.thread_id == other.thread_id && self.key == other.key
    }
}

impl Eq for KeyStore {}

impl PartialOrd for KeyStore {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        if self.thread_id == other.thread_id {
            return self.key.partial_cmp(&other.key);
        }

        return self.thread_id.partial_cmp(&other.thread_id);
    }
}

impl Ord for KeyStore {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        if self.thread_id == other.thread_id {
            return self.key.cmp(&other.key);
        }
        return self.thread_id.cmp(&other.thread_id);
    }
}

static mut KEYS: Vec<KeyStore> = Vec::new();
static mut RECYCLE_KEYS: Vec<usize> = Vec::new();
static KEY_COUNT: AtomicUsize = AtomicUsize::new(0);

impl<T, A: Allocator> ThreadLocal<T, A> {
    unsafe fn create_key() -> usize {
        unsafe extern "C" fn dtor<T, A: Allocator>(ptr: *mut u8) {
            if ptr.is_null() {
                return;
            }
            let ptr = ptr as *mut T;
            core::ptr::drop_in_place(ptr);
            A::deallocate(ptr as _);
        }

        let id: u64 = core::mem::transmute(std::thread::current().id());
        let key = KEY_COUNT.fetch_add(1, Ordering::SeqCst);

        let store = KeyStore {
            thread_id: id,
            key,
            value: 0,
            dtor: Some(dtor::<T, A>),
        };

        match KEYS.binary_search(&store) {
            Err(idx) => {
                KEYS.insert(idx, store);
            }
            // key already used
            Ok(_) => {
                // try to get from recycled keys
                if let Some(key) = RECYCLE_KEYS.pop() {
                    return key;
                } else {
                    // key overflow
                    panic!("thread local keys exceeded usize::MAX")
                }
            }
        }
        return key;
    }

    unsafe fn get_key(key: usize) -> *mut T {
        unsafe extern "C" fn dtor<T, A: Allocator>(ptr: *mut u8) {
            if ptr.is_null() {
                return;
            }
            let ptr = ptr as *mut T;
            core::ptr::drop_in_place(ptr);
            A::deallocate(ptr as _);
        }

        let thread_id: u64 = core::mem::transmute(std::thread::current().id());
        let store = KeyStore {
            thread_id,
            key,
            value: 0,
            dtor: Some(dtor::<T, A>),
        };

        match KEYS.binary_search(&store) {
            Ok(idx) => {
                let s = &KEYS[idx];
                return s.value as *mut T;
            }
            Err(idx) => {
                KEYS.insert(idx, store);

                return 0 as *mut T;
            }
        }
    }

    unsafe fn set_key(key: usize, value: *mut T) {
        unsafe extern "C" fn dtor<T, A: Allocator>(ptr: *mut u8) {
            if ptr.is_null() {
                return;
            }
            let ptr = ptr as *mut T;
            core::ptr::drop_in_place(ptr);
            A::deallocate(ptr as _);
        }

        let thread_id: u64 = core::mem::transmute(std::thread::current().id());
        let store = KeyStore {
            thread_id,
            key,
            value: value as usize,
            dtor: Some(dtor::<T, A>),
        };

        match KEYS.binary_search(&store) {
            Ok(idx) => {
                let s = &mut KEYS[idx];
                s.value = value as usize;
            }
            Err(idx) => {
                KEYS.insert(idx, store);
            }
        }
    }

    unsafe fn delete_key(key: usize) {
        for s in &mut KEYS {
            if s.key == key {
                let ptr = s.value as *mut u8;
                s.value = 0;

                if let Some(dtor) = s.dtor {
                    dtor(ptr);
                    s.dtor = None;
                }
            };
        }

        RECYCLE_KEYS.push(key);
    }
}
