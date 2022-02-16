use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::ptr::NonNull;
use std::sync::Arc;

use crate::coroutine_impl::Coroutine;
use crate::join::Join;
use mco_gen::get_local_data;

// thread local map storage
thread_local! {static LOCALMAP: LocalMap = RefCell::new(HashMap::default());}

/// coroutine local storage
pub struct CoroutineLocal {
    // current coroutine handle
    co: Coroutine,
    // when panic happens, we need to trigger the join here
    join: Arc<Join>,
    // real local data hash map
    local_data: LocalMap,
}

impl CoroutineLocal {
    /// create coroutine local storage
    pub fn new(co: Coroutine, join: Arc<Join>) -> Box<Self> {
        Box::new(CoroutineLocal {
            co,
            join,
            local_data: RefCell::new(HashMap::default()),
        })
    }

    // get the coroutine handle
    pub fn get_co(&self) -> &Coroutine {
        &self.co
    }

    // get the join handle
    pub fn get_join(&self) -> Arc<Join> {
        self.join.clone()
    }
}

#[inline]
pub fn get_co_local_data() -> Option<NonNull<CoroutineLocal>> {
    let ptr = get_local_data();
    #[allow(clippy::cast_ptr_alignment)]
        NonNull::new(ptr as *mut CoroutineLocal)
}

fn with<F: FnOnce(&LocalMap) -> R, R>(f: F) -> R {
    match get_co_local_data() {
        Some(v) => f(&(unsafe { v.as_ref() }.local_data)),
        None => LOCALMAP.with(|data| f(data)),
    }
}

pub type LocalMap = RefCell<HashMap<TypeId, Box<dyn Opaque>, BuildHasherDefault<IdHasher>>>;

pub trait Opaque {}

impl<T> Opaque for T {}

/// A key for local data stored in a coroutine.
///
/// This type is generated by the `coroutine_local!` macro and performs very
/// similarly to the `thread_local!` macro and `std::thread::LocalKey` types.
/// Data associated with a `LocalKey<T>` is stored inside of a coroutine,
/// and the data is destroyed when the coroutine is completed.
///
/// coroutine-local data requires the `'static` bound to ensure it lives long
/// enough. When a key is accessed for the first time the coroutine's data is
/// initialized with the provided initialization expression to the macro.
pub struct LocalKey<T> {
    // "private" fields which have to be public to get around macro hygiene, not
    // included in the stability story for this type. Can change at any time.
    #[doc(hidden)]
    pub __key: fn() -> TypeId,
    #[doc(hidden)]
    pub __init: fn() -> T,
}

pub struct IdHasher {
    id: u64,
}

impl Default for IdHasher {
    fn default() -> IdHasher {
        IdHasher { id: 0 }
    }
}

impl Hasher for IdHasher {
    fn write(&mut self, _bytes: &[u8]) {
        // TODO: need to do something sensible
        panic!("can only hash u64");
    }

    fn write_u64(&mut self, u: u64) {
        self.id = u;
    }

    fn finish(&self) -> u64 {
        self.id
    }
}

impl<T: 'static> LocalKey<T> {
    /// Access this coroutine-local key, running the provided closure with a
    /// reference to the value.
    ///
    /// This function will access this coroutine-local key to retrieve the data
    /// associated with the current coroutine and this key. If this is the first
    /// time this key has been accessed on this coroutine, then the key will be
    /// initialized with the initialization expression provided at the time the
    /// `coroutine_local!` macro was called.
    ///
    /// The provided closure will be provided a shared reference to the
    /// underlying data associated with this coroutine-local-key. The data itself
    /// is stored inside of the current coroutine.
    ///
    /// if it's not accessed in a coroutine context, it will use the thread local
    /// storage as a backend, so it's safe to use it in thread context
    ///
    /// # Panics
    ///
    /// This function can possibly panic for a number of reasons:
    ///
    /// * If the initialization expression is run and it panics
    /// * If the closure provided panics
    pub fn with<F, R>(&'static self, f: F) -> R
        where
            F: FnOnce(&T) -> R,
    {
        let key = (self.__key)();
        with(|data| {
            let raw_pointer = {
                let mut data = data.borrow_mut();
                let entry = data.entry(key).or_insert_with(|| Box::new((self.__init)()));
                &**entry as *const dyn Opaque as *const T
            };
            unsafe { f(&*raw_pointer) }
        })
    }
}
