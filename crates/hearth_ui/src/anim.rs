//! Damped springs: everything in the UI eases towards its target with a little overshoot.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringCfg {
    pub stiffness: f32,
    pub damping: f32,
}

impl SpringCfg {
    /// Pops with a visible overshoot (panels, popups, pressed buttons).
    pub const BOUNCY: SpringCfg = SpringCfg { stiffness: 320.0, damping: 15.0 };
    /// Quick and nearly critically damped (hover, scroll).
    pub const SNAPPY: SpringCfg = SpringCfg { stiffness: 520.0, damping: 38.0 };
    /// Jelly: overshoots several times (wobbles on value changes).
    pub const WOBBLY: SpringCfg = SpringCfg { stiffness: 260.0, damping: 7.0 };
    /// Slow and smooth (bars filling, counters).
    pub const GENTLE: SpringCfg = SpringCfg { stiffness: 90.0, damping: 18.0 };
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
}

impl Spring {
    pub fn new(x: f32) -> Spring {
        Spring { x, v: 0.0 }
    }

    /// Advances by `dt` seconds towards `target` (fixed substeps keep stiff springs stable).
    pub fn step(&mut self, target: f32, dt: f32, cfg: SpringCfg) {
        const H: f32 = 1.0 / 240.0;
        let mut left = dt.min(0.1);
        while left > 0.0 {
            let h = left.min(H);
            let a = -cfg.stiffness * (self.x - target) - cfg.damping * self.v;
            self.v += a * h;
            self.x += self.v * h;
            left -= h;
        }
        if (self.x - target).abs() < 1e-4 && self.v.abs() < 1e-3 {
            self.x = target;
            self.v = 0.0;
        }
    }
}

/// Smooth 0→1 ease for one-shot fades.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn springs_settle_and_overshoot() {
        let mut s = Spring::new(0.0);
        let mut peak: f32 = 0.0;
        for _ in 0..120 {
            s.step(1.0, 1.0 / 60.0, SpringCfg::BOUNCY);
            peak = peak.max(s.x);
        }
        assert!(peak > 1.02, "bouncy spring should overshoot, peak {peak}");
        assert!((s.x - 1.0).abs() < 1e-3);
    }
}
