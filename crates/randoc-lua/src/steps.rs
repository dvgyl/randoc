//! A count of the work steps of the algorithms that the time tests check
//! (standing order 21a). The count exists only in test builds; each
//! thread has its own, so tests that run at the same time do not mix.

#[cfg(test)]
thread_local! {
    static STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Adds `count` steps.
#[inline]
pub(crate) fn add(count: usize) {
    #[cfg(test)]
    STEPS.with(|steps| steps.set(steps.get().saturating_add(count as u64)));
    #[cfg(not(test))]
    let _ = count;
}

/// Returns the steps of this thread and sets the count to 0.
#[cfg(test)]
pub(crate) fn take() -> u64 {
    STEPS.with(|steps| steps.replace(0))
}
