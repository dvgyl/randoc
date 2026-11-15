//! A step counter for the time tests (standing order 21a): a test compares
//! the steps of an algorithm with a linear bound, so a busy machine cannot
//! make it fail. The counter counts only in test builds.

#[cfg(test)]
thread_local! {
    static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts `steps` steps of the current thread.
pub(crate) fn count(steps: usize) {
    #[cfg(test)]
    STEPS.with(|cell| cell.set(cell.get() + steps));
    #[cfg(not(test))]
    let _ = steps;
}

/// Returns the steps of the current thread since the last call and starts again from zero.
#[cfg(test)]
pub(crate) fn take() -> usize {
    STEPS.with(|cell| cell.replace(0))
}
