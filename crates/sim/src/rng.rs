//! Deterministic PCG32 random numbers. All sim randomness goes through [`SimRng`].

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimRng {
    state: u64,
    inc: u64,
}

impl SimRng {
    pub fn new(seed: u64) -> Self {
        let mut r = SimRng { state: 0, inc: (seed << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(seed ^ 0x9E37_79B9_7F4A_7C15);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// Uniform integer in [lo, hi] (inclusive).
    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() as u64 % (hi - lo + 1) as u64) as i64
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { ((self.next_u32() as u64 * n as u64) >> 32) as usize }
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() { None } else { Some(&items[self.below(items.len())]) }
    }

    /// Index chosen proportionally to non-negative weights. `None` if all weights are zero.
    pub fn weighted(&mut self, weights: &[f32]) -> Option<usize> {
        let total: f32 = weights.iter().map(|w| w.max(0.0)).sum();
        if total <= 0.0 {
            return None;
        }
        let mut t = self.f32() * total;
        for (i, w) in weights.iter().enumerate() {
            let w = w.max(0.0);
            if t < w {
                return Some(i);
            }
            t -= w;
        }
        Some(weights.len() - 1)
    }

    /// Derive an independent stream (e.g. per-peon generation).
    pub fn fork(&mut self) -> SimRng {
        SimRng::new(((self.next_u32() as u64) << 32) | self.next_u32() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_and_in_range() {
        let mut a = SimRng::new(42);
        let mut b = SimRng::new(42);
        for _ in 0..1000 {
            let x = a.f32();
            assert_eq!(x, b.f32());
            assert!((0.0..1.0).contains(&x));
            let i = a.range_i64(-3, 3);
            b.range_i64(-3, 3);
            assert!((-3..=3).contains(&i));
        }
        assert_ne!(SimRng::new(1).next_u32(), SimRng::new(2).next_u32());
    }
}
