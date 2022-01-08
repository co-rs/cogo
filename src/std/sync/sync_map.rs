use std::borrow::{Borrow, BorrowMut};
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::ops::{Deref, DerefMut};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, LockResult};
use std::time::Duration;
use crate::std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// this sync map used to many reader,writer less.
/// it's a mem space-for-time strategy
pub struct SyncMap<K, V> {
    dirty: RwLock<HashMap<K, V>>,
}


impl<K, V> SyncMap<K, V> where K: std::cmp::Eq + Hash + Clone {
    pub fn new() -> Self {
        Self {
            dirty: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            dirty: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }

    pub fn insert(&self, k: K, mut v: V) -> Option<V> where K: Clone {
        match self.dirty.write() {
            Ok(mut m) => {
                m.insert(k, v)
            }
            Err(_) => {
                Some(v)
            }
        }
    }

    pub fn remove(&self, k: &K) -> Option<V> where K: Clone {
        match self.dirty.write() {
            Ok(mut m) => {
                m.remove(k)
            }
            Err(_) => {
                None
            }
        }
    }

    pub fn len(&self) -> usize {
        loop {
            match self.dirty.read() {
                Ok(mut m) => {
                    return m.len();
                }
                Err(_) => {
                    continue;
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        loop {
            match self.dirty.read() {
                Ok(mut m) => {
                    return m.is_empty();
                }
                Err(_) => {
                    continue;
                }
            }
        }
    }

    pub fn clear(&self) {
        match self.dirty.write() {
            Ok(mut m) => {
                m.clear()
            }
            Err(_) => {}
        }
    }

    pub fn shrink_to_fit(&self) {
        match self.dirty.write() {
            Ok(mut m) => {
                m.shrink_to_fit()
            }
            Err(_) => {}
        }
    }

    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<SyncMapRef<'_, K, V>>
        where
            K: Borrow<Q>,
            Q: Hash + Eq,
    {
        let g = self.dirty.read();
        match g {
            Ok(mut m) => {
                let mut r = SyncMapRef {
                    g: m,
                    value: None,
                };
                unsafe {
                    r.value = Some(change_lifetime_const(r.g.get(k)?));
                }
                Some(r)
            }
            Err(_) => {
                None
            }
        }
    }

    pub fn get_mut<Q: ?Sized>(&self, k: &Q) -> Option<SyncMapRefMut<'_, K, V>>
        where
            K: Borrow<Q>,
            Q: Hash + Eq,
    {
        let g = self.dirty.write();
        match g {
            Ok(mut m) => {
                let mut r = SyncMapRefMut {
                    g: m,
                    value: None,
                };
                unsafe {
                    r.value = Some(change_lifetime_mut(r.g.get_mut(k)?));
                }
                Some(r)
            }
            Err(_) => {
                None
            }
        }
    }
}

pub unsafe fn change_lifetime_const<'a, 'b, T>(x: &'a T) -> &'b T {
    &*(x as *const T)
}

pub unsafe fn change_lifetime_mut<'a, 'b, T>(x: &'a mut T) -> &'b mut T {
    &mut *(x as *mut T)
}


pub struct SyncMapRef<'a, K, V> {
    g: RwLockReadGuard<'a, HashMap<K, V>>,
    value: Option<&'a V>,
}

impl<K, V> Deref for SyncMapRef<'_, K, V> {
    type Target = V;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().unwrap()
    }
}

impl<K, V> Debug for SyncMapRef<'_, K, V> where V: Debug {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}


impl<K, V> PartialEq<Self> for SyncMapRef<'_, K, V> where V: Eq {
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl<K, V> Eq for SyncMapRef<'_, K, V> where V: Eq {}


pub struct SyncMapRefMut<'a, K, V> {
    g: RwLockWriteGuard<'a, HashMap<K, V>>,
    value: Option<&'a mut V>,
}


impl<K, V> Deref for SyncMapRefMut<'_, K, V> {
    type Target = V;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().unwrap()
    }
}

impl<K, V> DerefMut for SyncMapRefMut<'_, K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().unwrap()
    }
}

impl<K, V> Debug for SyncMapRefMut<'_, K, V> where V: Debug {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}


impl<K, V> PartialEq<Self> for SyncMapRefMut<'_, K, V> where V: Eq {
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl<K, V> Eq for SyncMapRefMut<'_, K, V> where V: Eq {}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::ops::Deref;
    use std::sync::Arc;
    use std::sync::atomic::{Ordering};
    use crate::std::sync::{SyncMap, WaitGroup};


    #[test]
    pub fn test_empty() {
        let m: SyncMap<i32, i32> = SyncMap::new();
        assert_eq!(0, m.len());
    }

    #[test]
    pub fn test_insert() {
        let m = SyncMap::<i32, i32>::new();
        let insert = m.insert(1, 2);
        assert_eq!(insert.is_none(), true);
    }

    #[test]
    pub fn test_get() {
        let m = SyncMap::<i32, i32>::new();
        let insert = m.insert(1, 2);
        let g = m.get(&1).unwrap();
        assert_eq!(2, *g.deref());
    }

    #[test]
    pub fn test_smoke() {
        let wait1 = WaitGroup::new();
        let m1 = Arc::new(SyncMap::<i32, i32>::new());
        for i in 0..100 {
            let wg = wait1.clone();
            let m = m1.clone();
            go!(move ||{
                let insert = m.insert(1, 2);
                let g = m.get(&1).unwrap();
                assert_eq!(2, *g.deref());
                drop(wg);
                println!("done{}",i);
            });
        }
        for i in 0..100 {
            let wg = wait1.clone();
            let m = m1.clone();
            go!(move ||{
                let g = m.get(&2);
                assert_eq!(None, g);
                drop(wg);
                println!("done remove {}",i);
            });
        }
        wait1.wait();
    }
}