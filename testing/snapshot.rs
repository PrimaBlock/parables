use failure::Error;
use std::sync::Mutex;

/// A managed instance that can be shared by cloning across threads.
#[derive(Debug)]
pub struct Snapshot<T> {
    inner: Mutex<T>,
}

impl<T> Snapshot<T> {
    /// Create a new Snapshot value.
    pub fn new(inner: T) -> Self {
        Self {
            inner: Mutex::new(inner),
        }
    }

    /// Create a clone of the underlying value and return it.
    pub fn get(&self) -> Result<T, Error>
    where
        T: Clone,
    {
        let inner = self.inner.lock().map_err(|_| format_err!("lock poisoned"))?;
        Ok(inner.clone())
    }
}
