//! What the runs of one host share: run slots and a memory budget.
//!
//! A manifest's limits bound one run of one module; they do not bound the
//! host, which may run many modules at once (ADR 0003, « Limites »). Two
//! counters, shared by every clone of a [`crate::Host`] and every module it
//! loaded, do:
//! - run slots: a run past [`crate::HostLimits::max_concurrent_runs`] waits
//!   for another to end before it starts; its time limit starts with it;
//! - memory: every growth of a module's memory, of its tables and of its
//!   answer takes its bytes from the budget first. A growth the budget
//!   cannot cover stops the module that asked for it. The bytes return
//!   when the run is over.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

pub(crate) struct Shared {
    max_runs: usize,
    memory: usize,
    state: Mutex<State>,
    freed: Condvar,
}

#[derive(Default)]
struct State {
    runs: usize,
    held: usize,
}

impl Shared {
    pub(crate) fn new(max_runs: usize, memory: usize) -> Arc<Shared> {
        Arc::new(Shared {
            max_runs: max_runs.max(1),
            memory,
            state: Mutex::default(),
            freed: Condvar::new(),
        })
    }

    /// Nothing panics while the lock is held, so a poisoned lock still
    /// guards consistent counts.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wait for a run slot.
    pub(crate) fn enter(self: &Arc<Self>) -> Slot {
        let mut state = self.lock();
        while state.runs >= self.max_runs {
            state = self
                .freed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.runs += 1;
        Slot(Arc::clone(self))
    }

    /// Bytes held right now, all runs together: what a test waits on
    /// instead of guessing how long a module takes to grow.
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.lock().held
    }
}

/// A run slot, freed when dropped.
pub(crate) struct Slot(Arc<Shared>);

impl Drop for Slot {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.runs = state.runs.saturating_sub(1);
        drop(state);
        self.0.freed.notify_one();
    }
}

/// Bytes of the budget held for one run, given back when dropped.
pub(crate) struct Held {
    shared: Arc<Shared>,
    bytes: usize,
}

impl Held {
    pub(crate) fn new(shared: &Arc<Shared>) -> Held {
        Held {
            shared: Arc::clone(shared),
            bytes: 0,
        }
    }

    /// Take `bytes` more, or nothing when the budget cannot cover them.
    pub(crate) fn grow(&mut self, bytes: usize) -> bool {
        // `memory.grow(0)` in a loop must not make every run of the host
        // wait on this lock.
        if bytes == 0 {
            return true;
        }
        let mut state = self.shared.lock();
        match state
            .held
            .checked_add(bytes)
            .filter(|&n| n <= self.shared.memory)
        {
            Some(n) => {
                state.held = n;
                self.bytes += bytes;
                true
            }
            None => false,
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.held = state.held.saturating_sub(self.bytes);
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_shared_and_given_back() {
        let shared = Shared::new(4, 100);
        let mut a = Held::new(&shared);
        let mut b = Held::new(&shared);
        assert!(a.grow(60));
        assert!(!b.grow(41));
        assert!(b.grow(40));
        assert!(!a.grow(1));
        drop(a);
        assert!(b.grow(60));
        assert!(!b.grow(1));
        assert!(!Held::new(&shared).grow(usize::MAX));
        // Nothing asked, nothing refused, even with the budget used up.
        assert!(Held::new(&shared).grow(0));
    }

    #[test]
    fn a_run_past_the_slots_waits_for_one_to_end() {
        let shared = Shared::new(1, 0);
        let first = shared.enter();
        let (tx, rx) = std::sync::mpsc::channel();
        let waiting = {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                let _second = shared.enter();
                tx.send(()).unwrap();
            })
        };
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err());
        drop(first);
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        waiting.join().unwrap();
    }
}
