use std::sync::{Arc, Condvar, Mutex};

/// Thread-safe wrapper around a value that allows threads to wait for specific conditions on that value.
#[derive(Debug, Clone)]
pub struct Waitable<T> {
    inner: Arc<(Mutex<T>, Condvar)>,
}

impl<T> Waitable<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new((Mutex::new(value), Condvar::new())),
        }
    }

    pub fn set(&self, value: T) -> Result<(), std::sync::PoisonError<std::sync::MutexGuard<T>>> {
        let (mutex, condvar) = &*self.inner;
        let mut guard = mutex.lock()?;
        *guard = value;
        condvar.notify_all();
        Ok(())
    }

    pub fn get(&self) -> Result<T, std::sync::PoisonError<std::sync::MutexGuard<T>>>
    where
        T: Clone,
    {
        let (mutex, _) = &*self.inner;
        let guard = mutex.lock()?;
        Ok(guard.clone())
    }

    /// Waits until the value equals the given value
    pub fn wait_for(
        &self,
        expected: T,
    ) -> Result<T, std::sync::PoisonError<std::sync::MutexGuard<T>>>
    where
        T: Clone + PartialEq,
    {
        let (mutex, condvar) = &*self.inner;
        let guard = mutex.lock()?;
        let guard = condvar.wait_while(guard, |value| *value != expected)?;
        Ok(guard.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_basic_operations() {
        let waitable = Waitable::new(42);

        // Test get
        assert_eq!(waitable.get().unwrap(), 42);

        // Test set
        waitable.set(100).unwrap();
        assert_eq!(waitable.get().unwrap(), 100);
    }

    #[test]
    fn test_wait_for() {
        let waitable = Waitable::new(0);
        let waitable_clone = waitable.clone();

        // Spawn a thread that will change the value after a delay
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            waitable_clone.set(42).unwrap();
        });

        // Wait for the value to become 42
        let result = waitable.wait_for(42).unwrap();
        assert_eq!(result, 42);
    }
}
