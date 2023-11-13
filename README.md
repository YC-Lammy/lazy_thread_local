# lazy_thread_local
Lazy Per-object thread-local storage

This library provides the `ThreadLocal` type which allows a separate copy of
an object to be used for each thread. This allows for per-object
thread-local storage, unlike the crate `thread_local`, this crate provides
lazy initialisation and does not depend on std. 

Per-thread objects are not destroyed when a thread exits. Instead, objects
are only destroyed when the `ThreadLocal` containing them is dropped.

This crate uses platform dependent methods to create thread local keys.
On Unix, pthread local storage is used. On windows, Fibers storage is used.
On wasm, it relies on std to provide thread id.

# Examples

Basic usage of `ThreadLocal`:

```rust
use lazy_thread_local::ThreadLocal;
let mut tls: ThreadLocal<u32> = ThreadLocal::new(||5);
assert_eq!(tls.get(), &5);
*tls = 6;
assert_eq!(tls.get(), &6);
```

Initialising `ThreadLocal` in constant context:

```rust
use lazy_thread_local::ThreadLocal;

static TLS: ThreadLocal<u32> = ThreadLocal::const_new(5);

assert_eq!(TLS.get(), 5);
```
