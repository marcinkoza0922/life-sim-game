use glam::Vec3;
use hearth_render::Camera;
use sim::lot::LEVEL_HEIGHT;

/// Dollhouse orbit camera.
#[derive(Clone, Copy, Debug)]
pub struct OrbitCamera {
    /// Ground-plane focus (y is derived from the active level).
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    /// Smoothed values actually used for rendering.
    pub smooth_target: Vec3,
    pub smooth_yaw: f32,
    pub smooth_pitch: f32,
    pub smooth_dist: f32,
}

impl OrbitCamera {
    pub fn new(target: Vec3) -> Self {
        let (yaw, pitch, dist) = (0.6, 0.75, 26.0);
        OrbitCamera { target, yaw, pitch, dist, smooth_target: target, smooth_yaw: yaw, smooth_pitch: pitch, smooth_dist: dist }
    }

    pub fn focus(&self, level: i32) -> Vec3 {
        Vec3::new(self.smooth_target.x, level as f32 * LEVEL_HEIGHT + 0.5, self.smooth_target.z)
    }

    pub fn camera(&self, level: i32, aspect: f32) -> Camera {
        Camera::orbit(self.focus(level), self.smooth_yaw, self.smooth_pitch, self.smooth_dist, aspect)
    }

    pub fn update(&mut self, dt: f32) {
        let k = 1.0 - (-dt * 14.0).exp();
        self.smooth_target = self.smooth_target.lerp(self.target, k);
        self.smooth_yaw += (self.yaw - self.smooth_yaw) * k;
        self.smooth_pitch += (self.pitch - self.smooth_pitch) * k;
        self.smooth_dist += (self.dist - self.smooth_dist) * k;
    }

    pub fn snap(&mut self) {
        self.smooth_target = self.target;
        self.smooth_yaw = self.yaw;
        self.smooth_pitch = self.pitch;
        self.smooth_dist = self.dist;
    }

    /// Pan in screen-aligned ground directions.
    pub fn pan(&mut self, right: f32, forward: f32) {
        let f = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let r = Vec3::new(-f.z, 0.0, f.x);
        let s = self.dist * 0.04;
        self.target += r * right * s + f * forward * s;
        self.target.x = self.target.x.clamp(-10.0, 50.0);
        self.target.z = self.target.z.clamp(-10.0, 50.0);
    }

    pub fn rotate(&mut self, dyaw: f32, dpitch: f32) {
        self.yaw += dyaw;
        self.pitch = (self.pitch + dpitch).clamp(0.15, 1.45);
    }

    pub fn zoom(&mut self, factor: f32) {
        self.dist = (self.dist * factor).clamp(4.0, 90.0);
    }
}
