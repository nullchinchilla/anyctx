use std::{
    any::Any,
    mem::MaybeUninit,
    ops::Deref,
    sync::{Arc, RwLock},
};

use ahash::AHashMap;

#[derive(Clone)]
/// A context type for storing and retrieving "quasi-global" data in a type-safe and scope-respecting manner. Think of it as a "god object" that does not clutter up scope or generate spaghetti.
///
/// This context allows for the dynamic association of data with a key derived from a lazily evaluated constructor. It is designed to be thread-safe and can be shared across threads.
///
/// Moreover, the context also wraps a provided *initialization value*. This allows easy access to data that's more ergonomically passed in at initialization rather than lazily initialized later.
///
/// Generics:
/// - `I`: Initialization info type, which must be `Send + Sync + 'static`
///
/// # Examples
///
/// ```
/// use anyctx::AnyCtx;
///
/// fn forty_two(ctx: &AnyCtx<i32>) -> i32 {
///     40 + *ctx.init()
/// }
///
/// let ctx = AnyCtx::new(2);
/// let number = ctx.get(forty_two);
/// assert_eq!(*number, 42);
/// ```
pub struct AnyCtx<I: Send + Sync + 'static> {
    init: Arc<I>,
    dynamic: Arc<RwLock<AHashMap<*const (), Arc<RwLock<MaybeUninit<Box<dyn Any + Send + Sync>>>>>>>,
}

unsafe impl<T: Send + Sync + 'static> Send for AnyCtx<T> {}
unsafe impl<T: Send + Sync + 'static> Sync for AnyCtx<T> {}

impl<I: Send + Sync + 'static> AnyCtx<I> {
    /// Creates a new context, wrapping the given initialization value.
    pub fn new(init: I) -> Self {
        Self {
            init: init.into(),
            dynamic: Default::default(),
        }
    }

    /// Gets the initialization value.
    pub fn init(&self) -> &I {
        &self.init
    }

    /// Gets the value associated with the given constructor function. If there already is a value, the value will be retrieved. Otherwise, the constructor will be run to produce the value, which will be stored in the context.
    ///
    /// It is guaranteed that the constructor will be called at most once, even if `get` is called concurrently from multiple threads with the same key.
    ///
    /// The constructor itself should take in an AnyCtx as an argument, and is allowed to call `get` too. Take care to avoid infinite recursion, which will cause a deadlock.
    pub fn get<T: 'static + Send + Sync>(&self, construct: fn(&Self) -> T) -> &T {
        loop {
            if let Some(exists) = self.get_inner(construct) {
                return exists;
            } else {
                let key: *const () = construct as *const ();
                let mut inner = self.dynamic.write().unwrap();
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
                let value = construct(self);
                entry.write(Box::new(value));
                // loop around again and we'll get it
            }
        }
    }

    fn get_inner<'a, T: 'static + Send + Sync>(&'a self, init: fn(&'a Self) -> T) -> Option<&'a T> {
        let inner = self.dynamic.read().unwrap();
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
    use crate::AnyCtx;

    fn one(_ctx: &AnyCtx<()>) -> usize {
        1
    }

    fn hello(_ctx: &AnyCtx<()>) -> String {
        "hello".to_string()
    }

    fn two(ctx: &AnyCtx<()>) -> usize {
        ctx.get(one) + ctx.get(one)
    }

    #[test]
    fn simple() {
        let ctx = AnyCtx::new(());
        assert_eq!(ctx.get(two), &2);
        assert_eq!(ctx.get(hello), "hello")
    }
}
