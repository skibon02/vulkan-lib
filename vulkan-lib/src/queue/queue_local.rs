use std::cell::UnsafeCell;
use std::sync::atomic::AtomicBool;

static TOKEN_CREATED: AtomicBool = AtomicBool::new(false);
pub(crate) struct QueueLocalToken;
impl QueueLocalToken {
    #[must_use]
    pub(crate) fn try_new() -> Option<Self> {
        if !TOKEN_CREATED.swap(true, std::sync::atomic::Ordering::Acquire) {
            Some(QueueLocalToken)
        }
        else {
            None
        }
    }
}

impl Drop for QueueLocalToken {
    fn drop(&mut self) {
        TOKEN_CREATED.store(false, std::sync::atomic::Ordering::Release);
    }
}

pub struct QueueLocal<T> {
    inner: UnsafeCell<T>
}

impl<T> QueueLocal<T> {
    pub fn new(value: T) -> Self {
        QueueLocal {
            inner: UnsafeCell::new(value)
        }
    }

    pub fn get<'a>(&'a self, _token: &'a mut QueueLocalToken) -> &'a mut T {
        unsafe { &mut *self.inner.get() }
    }
    pub fn get_owned(&mut self) -> &mut T {
        self.inner.get_mut()
    }
}

unsafe impl<T: Send> Send for QueueLocal<T> {}
unsafe impl<T: Send> Sync for QueueLocal<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_massive_concurrent_increments() {
        use std::sync::Arc;
        use std::thread;

        const NUM_THREADS: usize = 1000;
        let queue_local = Arc::new(QueueLocal::new(0u64));

        let handles: Vec<_> = (0..NUM_THREADS).map(|_| {
            let ql = queue_local.clone();
            thread::spawn(move || {
                loop {
                    if let Some(mut token) = QueueLocalToken::try_new() {
                        let val = ql.get(&mut token);
                        *val += 1;
                        drop(token);
                        break;
                    }
                    std::hint::spin_loop();
                }
            })
        }).collect();

        for handle in handles {
            handle.join().unwrap();
        }

        if let Some(mut token) = QueueLocalToken::try_new() {
            let final_val = *queue_local.get(&mut token);
            println!("Final value: {}, expected: {}", final_val, NUM_THREADS);

            assert_eq!(final_val, NUM_THREADS as u64,
                "Lost increments detected! This indicates a memory ordering issue.");
        } else {
            panic!("Token should be available after all threads completed");
        }
    }
}