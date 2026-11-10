//! Persistent graphics preferences, independent of simulation saves and audio preferences.
use hearth_render::LightingOptions;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    pub ambient_occlusion: bool,
    pub indirect_light: bool,
    pub shadows: bool,
    pub soft_shadows: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        let o = LightingOptions::default();
        Self { ambient_occlusion: o.ambient_occlusion, indirect_light: o.indirect_light, shadows: o.shadows, soft_shadows: o.soft_shadows }
    }
}

impl GraphicsSettings {
    pub fn load() -> Self {
        std::fs::read_to_string("graphics.ron").ok().and_then(|s| ron::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = ron::ser::to_string_pretty(self, Default::default())
            && let Err(e) = std::fs::write("graphics.ron", s)
        {
            log::warn!("couldn't save graphics settings: {e}");
        }
    }

    pub fn lighting(self) -> LightingOptions {
        LightingOptions { ambient_occlusion: self.ambient_occlusion, indirect_light: self.indirect_light, shadows: self.shadows, soft_shadows: self.soft_shadows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_and_missing_fields_default() {
        let settings = GraphicsSettings { indirect_light: true, shadows: false, ..Default::default() };
        let restored: GraphicsSettings = ron::from_str(&ron::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        let old: GraphicsSettings = ron::from_str("(shadows:false)").unwrap();
        assert!(!old.shadows);
        assert_eq!(old.ambient_occlusion, GraphicsSettings::default().ambient_occlusion);
    }
}
