use std::{
    any::Any,
    mem::MaybeUninit,
    sync::{Arc, RwLock},
};

use ahash::AHashMap;

#[derive(Default, Clone)]
pub struct Context {
    inner: Arc<RwLock<AHashMap<*const (), Arc<RwLock<MaybeUninit<Box<dyn Any + Send + Sync>>>>>>>,
}

unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl Context {
    /// Creates a new context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Gets the value associated with the given initialization function.
    pub fn get_or_init<T: 'static + Send + Sync>(&self, init: fn(&Self) -> T) -> &T {
        loop {
            if let Some(exists) = self.get(init) {
                return exists;
            } else {
                let key: *const () = init as *const ();
                let mut inner = self.inner.write().unwrap();
                if inner.contains_key(&key) {
                    // now get will return, so loop around
                    continue;
                }
                let to_init = Arc::new(RwLock::new(MaybeUninit::uninit()));
                let mut entry = to_init.write().unwrap();
                inner.insert(key, to_init.clone());
                drop(inner);

                // now inner is unlocked, so we're not blocking the whole map.
                // but our particular entry is locked.
                // we can init in peace.
                let value = init(self);
                entry.write(Box::new(value));
                // loop around again and we'll get it
            }
        }
    }

    fn get<'a, T: 'static + Send + Sync>(&'a self, init: fn(&'a Self) -> T) -> Option<&'a T> {
        let inner = self.inner.read().unwrap();
        let b = inner.get(&(init as *const ()))?;
        let b = b.read().unwrap();
        // SAFETY: by the time we can read-lock this value, we know that it has already initialized, since the initialization function holds a lock for the full duration.
        let b = unsafe { b.assume_init_ref() };
        let downcasted: &T = b
            .downcast_ref()
            .expect("downcast failed, this should not happen");
        // SAFETY: we never remove items from inner without dropping Context first, and the address of what Box points to cannot change, so this is safe
        let downcasted: &'a T = unsafe { std::mem::transmute(downcasted) };
        Some(downcasted)
    }
}

#[cfg(test)]
mod tests {
    use crate::Context;

    fn one(_ctx: &Context) -> usize {
        1
    }

    fn hello(_ctx: &Context) -> String {
        "hello".to_string()
    }

    fn two(ctx: &Context) -> usize {
        ctx.get_or_init(one) + ctx.get_or_init(one)
    }

    #[test]
    fn simple() {
        let ctx = Context::new();
        assert_eq!(ctx.get_or_init(two), &2);
        assert_eq!(ctx.get_or_init(hello), "hello")
    }
}
