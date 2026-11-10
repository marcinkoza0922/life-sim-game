//! Opt-in coarse CPU attribution. Inclusive timings overlap for nested spans.
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default)]
pub struct Metric {
    pub calls: u64,
    pub elapsed: Duration,
}

thread_local! {
    static METRICS: RefCell<[Metric; 2]> = RefCell::new([Metric::default(); 2]);
}

pub const NAMES: [&str; 2] = ["navigation", "autonomy"];

/// Reset after setup/warmup; counters are local to the calling simulation thread.
pub fn reset() {
    METRICS.with(|m| *m.borrow_mut() = [Metric::default(); 2]);
}

pub fn snapshot() -> [Metric; 2] {
    METRICS.with(|m| *m.borrow())
}

pub(crate) struct Span {
    index: usize,
    start: Instant,
}

impl Span {
    pub(crate) fn new(index: usize) -> Self {
        Self {
            index,
            start: Instant::now(),
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        METRICS.with(|m| {
            let mut metrics = m.borrow_mut();
            metrics[self.index].calls += 1;
            metrics[self.index].elapsed += elapsed;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_spans_count_and_reset() {
        reset();
        {
            let _outer = Span::new(1);
            let _inner = Span::new(0);
        }
        let metrics = snapshot();
        assert_eq!(metrics[0].calls, 1);
        assert_eq!(metrics[1].calls, 1);
        assert!(metrics[1].elapsed >= metrics[0].elapsed);
        reset();
        assert!(
            snapshot()
                .iter()
                .all(|m| m.calls == 0 && m.elapsed.is_zero())
        );
    }
}
