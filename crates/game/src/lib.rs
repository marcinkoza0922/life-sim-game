//! Shared game-side code: scene building (sim → renderer), camera, game state and UI.
//! Used by the `lifesim` binary and by `simctl render`.

pub mod audio;
pub mod camera;
pub mod creator;
pub mod lotmesh;
pub mod scene;
pub mod state;
pub mod ui;
