use crate::std::sync::{Mutex, MutexGuard};
use serde::ser::SerializeMap;
use serde::{Deserializer, Serialize, Serializer};
use std::borrow::Borrow;
use std::cell::UnsafeCell;
use std::collections::{btree_map::Iter as MapIter, BTreeMap as Map, HashMap};
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub type SyncBtreeMap<K, V> = SyncBtreeMapImpl<K, V>;

/// this sync map used to many reader,writer less.space-for-time strategy
///
/// Map is like a Go map[interface{}]interface{} but is safe for concurrent use
/// by multiple goroutines without additional locking or coordination.
/// Loads, stores, and deletes run in amortized constant time.
///
/// The Map type is specialized. Most code should use a plain Go map instead,
/// with separate locking or coordination, for better type safety and to make it
/// easier to maintain other invariants along with the map content.
///
/// The Map type is optimized for two common use cases: (1) when the entry for a given
/// key is only ever written once but read many times, as in caches that only grow,
/// or (2) when multiple goroutines read, write, and overwrite entries for disjoint
/// sets of keys. In these two cases, use of a Map may significantly reduce lock
/// contention compared to a Go map paired with a separate Mutex or RWMutex.
///
/// The zero Map is empty and ready for use. A Map must not be copied after first use.
pub struct SyncBtreeMapImpl<K: Eq + Hash + Clone + Ord, V> {
    read: UnsafeCell<Map<K, V>>,
    dirty: Mutex<HashMap<K, V>>,
}

impl<K: Eq + Hash + Clone + Ord, V> Drop for SyncBtreeMapImpl<K, V> {
    fn drop(&mut self) {
        unsafe {
            let mut keys = Vec::with_capacity(self.len());
            for (k, _) in &mut *self.read.get() {
                keys.insert(0, k);
            }
            for x in keys {
                let v = (&mut *self.read.get()).remove(x);
                match v {
                    None => {}
                    Some(v) => {
                        std::mem::forget(v);
                    }
                }
            }
        }
    }
}

/// this is safety, dirty mutex ensure
unsafe impl<K: Eq + Hash + Clone + Ord, V> Send for SyncBtreeMapImpl<K, V> {}

/// this is safety, dirty mutex ensure
unsafe impl<K: Eq + Hash + Clone + Ord, V> Sync for SyncBtreeMapImpl<K, V> {}

//TODO maybe K will use transmute_copy replace Clone?
impl<K: Eq + Hash + Clone + Ord, V> SyncBtreeMapImpl<K, V>
where
    K: std::cmp::Eq + Hash + Clone,
{
    pub fn new_arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn new() -> Self {
        Self {
            read: UnsafeCell::new(Map::new()),
            dirty: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_capacity(_capacity: usize) -> Self {
        Self::new()
    }

    pub fn insert(&self, k: K, v: V) -> Option<V>
    where
        K: Clone + std::cmp::Ord,
    {
        match self.dirty.lock() {
            Ok(mut m) => {
                let op = m.insert(k.clone(), v);
                match op {
                    None => {
                        let r = m.get(&k);
                        unsafe {
                            (&mut *self.read.get()).insert(k, std::mem::transmute_copy(r.unwrap()));
                        }
                        None
                    }
                    Some(v) => Some(v),
                }
            }
            Err(_) => Some(v),
        }
    }

    pub fn remove(&self, k: &K) -> Option<V>
    where
        K: Clone + std::cmp::Ord,
    {
        match self.dirty.lock() {
            Ok(mut m) => {
                let op = m.remove(k);
                match op {
                    Some(v) => {
                        unsafe {
                            let r = (&mut *self.read.get()).remove(k);
                            match r {
                                None => {}
                                Some(r) => {
                                    std::mem::forget(r);
                                }
                            }
                        }
                        Some(v)
                    }
                    None => None,
                }
            }
            Err(_) => None,
        }
    }

    pub fn len(&self) -> usize {
        unsafe { (&*self.read.get()).len() }
    }

    pub fn is_empty(&self) -> bool {
        unsafe { (&*self.read.get()).is_empty() }
    }

    pub fn clear(&self)
    where
        K: std::cmp::Eq + Hash + Clone + std::cmp::Ord,
    {
        match self.dirty.lock() {
            Ok(mut m) => {
                m.clear();
                unsafe {
                    let k = (&mut *self.read.get()).keys().clone();
                    for x in k {
                        let v = (&mut *self.read.get()).remove(x);
                        match v {
                            None => {}
                            Some(v) => {
                                std::mem::forget(v);
                            }
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }

    pub fn shrink_to_fit(&self) {}

    pub fn from(map: HashMap<K, V>) -> Self
    where
        K: Clone + Eq + Hash + std::cmp::Ord,
    {
        let s = Self::new();
        match s.dirty.lock() {
            Ok(mut m) => {
                *m = map;
                unsafe {
                    for (k, v) in m.iter() {
                        (&mut *s.read.get()).insert(k.clone(), std::mem::transmute_copy(v));
                    }
                }
            }
            Err(_) => {}
        }
        s
    }

    /// Returns a reference to the value corresponding to the key.
    ///
    /// The key may be any borrowed form of the map's key type, but
    /// [`Hash`] and [`Eq`] on the borrowed form *must* match those for
    /// the key type.
    ///
    /// Since reading a map is unlocked, it is very fast
    ///
    /// test bench_sync_hash_map_read   ... bench:           8 ns/iter (+/- 0)
    /// # Examples
    ///
    /// ```
    /// use mco::std::sync::{SyncHashMap};
    ///
    /// let map = SyncHashMap::new();
    /// map.insert(1, "a");
    /// assert_eq!(*map.get(&1).unwrap(), "a");
    /// assert_eq!(map.get(&2).is_none(), true);
    /// ```
    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q> + std::cmp::Ord,
        Q: Hash + Eq + std::cmp::Ord,
    {
        unsafe {
            let k = (&*self.read.get()).get(k);
            match k {
                None => None,
                Some(s) => Some(s),
            }
        }
    }

    pub fn get_mut<Q: ?Sized>(&self, k: &Q) -> Option<SyncBtreeMapRefMut<'_, K, V>>
    where
        K: Borrow<Q> + std::cmp::Ord,
        Q: Hash + Eq + std::cmp::Ord,
    {
        let g = self.dirty.lock();
        match g {
            Ok(m) => {
                let mut r = SyncBtreeMapRefMut { g: m, value: None };
                unsafe {
                    r.value = Some(change_lifetime_mut(r.g.get_mut(k)?));
                }
                Some(r)
            }
            Err(_) => None,
        }
    }

    pub fn iter(&self) -> MapIter<'_, K, V> {
        unsafe { (&*self.read.get()).iter() }
    }

    pub fn iter_mut(&self) -> IterBtreeMut<'_, K, V> {
        loop {
            match self.dirty.lock() {
                Ok(m) => {
                    let mut iter = IterBtreeMut { g: m, inner: None };
                    unsafe {
                        iter.inner = Some(change_lifetime_mut(&mut iter.g).iter_mut());
                    }
                    return iter;
                }
                Err(_) => {
                    continue;
                }
            }
        }
    }

    pub fn into_iter(self) -> MapIter<'static, K, V> {
        unsafe { (&*self.read.get()).iter() }
    }
}

pub unsafe fn change_lifetime_const<'a, 'b, T>(x: &'a T) -> &'b T {
    &*(x as *const T)
}

unsafe fn change_lifetime_mut<'a, 'b, T>(x: &'a mut T) -> &'b mut T {
    &mut *(x as *mut T)
}

pub struct SyncBtreeMapRefMut<'a, K, V> {
    g: MutexGuard<'a, HashMap<K, V>>,
    value: Option<&'a mut V>,
}

impl<'a, K, V> Deref for SyncBtreeMapRefMut<'_, K, V> {
    type Target = V;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().unwrap()
    }
}

impl<'a, K, V> DerefMut for SyncBtreeMapRefMut<'_, K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().unwrap()
    }
}

impl<'a, K, V> Debug for SyncBtreeMapRefMut<'_, K, V>
where
    V: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}

impl<'a, K, V> PartialEq<Self> for SyncBtreeMapRefMut<'_, K, V>
where
    V: Eq,
{
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl<'a, K, V> Eq for SyncBtreeMapRefMut<'_, K, V> where V: Eq {}

pub struct IterBtree<'a, K, V> {
    inner: Option<MapIter<'a, K, *const V>>,
}

impl<'a, K, V> Iterator for IterBtree<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.inner.as_mut().unwrap().next();
        match next {
            None => None,
            Some((k, v)) => {
                if v.is_null() {
                    None
                } else {
                    unsafe { Some((k, &**v)) }
                }
            }
        }
    }
}

pub struct IterBtreeMut<'a, K, V> {
    g: MutexGuard<'a, HashMap<K, V>>,
    inner: Option<std::collections::hash_map::IterMut<'a, K, V>>,
}

impl<'a, K, V> Deref for IterBtreeMut<'a, K, V> {
    type Target = std::collections::hash_map::IterMut<'a, K, V>;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().unwrap()
    }
}

impl<'a, K, V> DerefMut for IterBtreeMut<'a, K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut().unwrap()
    }
}

impl<'a, K, V> Iterator for IterBtreeMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.as_mut().unwrap().next()
    }
}

impl<'a, K: Eq + Hash + Clone + Ord, V> IntoIterator for &'a SyncBtreeMapImpl<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = MapIter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, K: Eq + Hash + Clone + Ord, V> IntoIterator for &'a mut SyncBtreeMapImpl<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = IterBtreeMut<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<K: Eq + Hash + Clone + Ord, V> IntoIterator for SyncBtreeMapImpl<K, V>
where
    K: Eq + Hash + Clone,
    K: 'static,
    V: 'static,
{
    type Item = (&'static K, &'static V);
    type IntoIter = MapIter<'static, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_iter()
    }
}

impl<K: Eq + Hash + Clone + Ord, V> From<HashMap<K, V>> for SyncBtreeMapImpl<K, V> {
    fn from(arg: HashMap<K, V>) -> Self {
        Self::from(arg)
    }
}

impl<K: Eq + Hash + Clone + Ord, V> serde::Serialize for SyncBtreeMapImpl<K, V>
where
    K: Eq + Hash + Clone + Serialize,
    V: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut m = serializer.serialize_map(Some(self.len()))?;
        for (k, v) in self.iter() {
            m.serialize_key(k)?;
            m.serialize_value(v)?;
        }
        m.end()
    }
}

impl<'de, K, V> serde::Deserialize<'de> for SyncBtreeMapImpl<K, V>
where
    K: Eq + Hash + Clone + serde::Deserialize<'de> + std::cmp::Ord,
    V: serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let m = HashMap::deserialize(deserializer)?;
        Ok(Self::from(m))
    }
}

impl<K: Eq + Hash + Clone + Ord, V> Debug for SyncBtreeMapImpl<K, V>
where
    K: std::cmp::Eq + Hash + Clone + Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut m = f.debug_map();
        for (k, v) in self.iter() {
            m.key(k);
            m.value(v);
        }
        m.finish()
    }
}

#[cfg(test)]
mod test {
    use crate::std::sync::SyncBtreeMap;
    use crate::std::sync::WaitGroup;
    use std::collections::BTreeMap;
    use std::ops::Deref;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    #[test]
    pub fn test_empty() {
        let m: SyncBtreeMap<i32, i32> = SyncBtreeMap::new();
        assert_eq!(0, m.len());
    }

    #[test]
    pub fn test_insert() {
        let m = SyncBtreeMap::<i32, i32>::new();
        let insert = m.insert(1, 2);
        assert_eq!(insert.is_none(), true);
    }

    #[test]
    pub fn test_insert2() {
        let m = Arc::new(SyncBtreeMap::<String, String>::new());
        m.insert("/".to_string(), "1".to_string());
        m.insert("/js".to_string(), "2".to_string());
        m.insert("/fn".to_string(), "3".to_string());

        assert_eq!(&"1".to_string(), m.get("/").unwrap());
        assert_eq!(&"2".to_string(), m.get("/js").unwrap());
        assert_eq!(&"3".to_string(), m.get("/fn").unwrap());
    }

    #[test]
    pub fn test_insert3() {
        let m = Arc::new(SyncBtreeMap::<i32, i32>::new());
        let wg = WaitGroup::new();
        for _ in 0..100000 {
            let wg1 = wg.clone();
            let wg2 = wg.clone();
            let m1 = m.clone();
            let m2 = m.clone();
            co!(move || {
                m1.remove(&1);
                let insert = m1.insert(1, 2);
                drop(wg1);
            });
            co!(move || {
                m2.remove(&1);
                let insert = m2.insert(1, 2);
                drop(wg2);
            });
        }
        wg.wait();
    }

    #[test]
    pub fn test_get() {
        let m = SyncBtreeMap::<i32, i32>::new();
        let insert = m.insert(1, 2);
        let g = m.get(&1).unwrap();
        assert_eq!(2, *g.deref());
    }

    #[test]
    pub fn test_iter() {
        let m = SyncBtreeMap::<i32, i32>::new();
        let insert = m.insert(1, 2);
        for (k, v) in m.iter() {
            assert_eq!(*k, 1);
            assert_eq!(*v, 2);
        }
    }

    #[test]
    pub fn test_iter_mut() {
        let m = SyncBtreeMap::<i32, i32>::new();
        let insert = m.insert(1, 2);
        for (k, v) in m.iter_mut() {
            assert_eq!(*k, 1);
            assert_eq!(*v, 2);
        }
    }
}
