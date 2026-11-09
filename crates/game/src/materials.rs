//! Loads the embedded material textures (see `build.rs`) into the renderer, for every
//! [`sim::data::MaterialDef`] that has them. Ids without textures stay flat.

use hearth_render::{MaterialMaps, Renderer};
use sim::data::GameData;

include!(concat!(env!("OUT_DIR"), "/textures.rs"));

fn decode(id: &str, map: &str, bytes: &[u8]) -> Option<image::RgbaImage> {
    match image::load_from_memory(bytes) {
        Ok(img) => Some(img.into_rgba8()),
        Err(e) => {
            log::warn!("material {id}: bad {map}.png: {e}");
            None
        }
    }
}

/// Decodes the maps of every defined material that has textures.
pub fn load(data: &GameData) -> Vec<(String, MaterialMaps)> {
    let mut out = Vec::new();
    for def in &data.materials {
        let Some(&(_, albedo, normal, orm)) = TEXTURES.iter().find(|t| t.0 == def.id) else { continue };
        let Some(albedo) = decode(&def.id, "albedo", albedo) else { continue };
        let normal = normal.and_then(|b| decode(&def.id, "normal", b));
        let orm = orm.and_then(|b| decode(&def.id, "orm", b));
        out.push((def.id.clone(), MaterialMaps { albedo, normal, orm, scale: def.scale }));
    }
    for (id, ..) in TEXTURES.iter().filter(|t| !data.materials.iter().any(|m| m.id == t.0)) {
        log::warn!("assets/textures/{id} isn't listed in materials.ron; ignored");
    }
    out
}

/// Registers the materials with the renderer.
pub fn install(r: &mut Renderer, data: &GameData) {
    let mats = load(data);
    log::info!("materials: {} textured of {} defined", mats.len(), data.materials.len());
    r.set_materials(&mats);
}

#[cfg(test)]
mod tests {
    use sim::data::GameData;

    #[test]
    fn embedded_textures_decode_and_are_defined() {
        let data = GameData::embedded();
        let mats = super::load(&data);
        assert_eq!(mats.len(), super::TEXTURES.len(), "every embedded texture set is defined and decodes");
    }
}
