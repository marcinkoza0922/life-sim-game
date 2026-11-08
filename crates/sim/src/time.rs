use serde::{Deserialize, Serialize};

pub const TICKS_PER_HOUR: u64 = 60;
pub const TICKS_PER_DAY: u64 = 24 * TICKS_PER_HOUR;
pub const START_HOUR: u64 = 7;

/// Game clock. One tick is one game minute.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Clock {
    pub tick: u64,
}

impl Clock {
    /// Absolute minutes since day 0 midnight.
    pub fn minutes(&self) -> u64 {
        self.tick + START_HOUR * TICKS_PER_HOUR
    }
    pub fn day(&self) -> u64 {
        self.minutes() / TICKS_PER_DAY
    }
    pub fn weekday(&self) -> u8 {
        (self.day() % 7) as u8
    }
    pub fn minute_of_day(&self) -> u64 {
        self.minutes() % TICKS_PER_DAY
    }
    pub fn hour(&self) -> u32 {
        (self.minute_of_day() / 60) as u32
    }
    /// Fractional hour of day, e.g. 13.5.
    pub fn hour_f(&self) -> f32 {
        self.minute_of_day() as f32 / 60.0
    }
    pub fn is_night(&self) -> bool {
        let h = self.hour();
        !(6..22).contains(&h)
    }
    pub fn weekday_name(&self) -> &'static str {
        ["Moonday", "Toosday", "Wendsday", "Thirstday", "Fryday", "Satyrday", "Snoozeday"][self.weekday() as usize]
    }
    pub fn label(&self) -> String {
        let m = self.minute_of_day();
        format!("Day {} ({}) {:02}:{:02}", self.day() + 1, self.weekday_name(), m / 60, m % 60)
    }
    /// Absolute tick at which the given minute-of-day next occurs (strictly after now).
    pub fn next_at(&self, minute_of_day: u64) -> u64 {
        let now = self.minute_of_day();
        let delta = if minute_of_day > now { minute_of_day - now } else { TICKS_PER_DAY - now + minute_of_day };
        self.tick + delta
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FreeWill {
    Off,
    Low,
    #[default]
    Full,
}

/// Player-adjustable simulation settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// Multiplies every life-stage duration.
    pub lifespan_scale: f32,
    pub aging: bool,
    pub free_will: FreeWill,
    pub accidents: bool,
    pub bills: bool,
    /// Resolve career event choices automatically (headless runs, or players who don't care).
    pub auto_career_choices: bool,
    /// Write a Debug log line for every action start/finish (for `simctl run --trace`).
    pub trace_actions: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lifespan_scale: 1.0,
            aging: true,
            free_will: FreeWill::Full,
            accidents: true,
            bills: true,
            auto_career_choices: false,
            trace_actions: false,
        }
    }
}
