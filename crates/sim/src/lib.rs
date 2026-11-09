//! The life-sim simulation: deterministic, headless game logic on the Hearth ECS.

pub mod actions;
pub mod autonomy;
pub mod career;
pub mod components;
pub mod data;
pub mod economy;
pub mod family;
pub mod genetics;
pub mod ghosts;
pub mod life;
pub mod log;
pub mod lot;
pub mod nav;
pub mod needs;
pub mod neighborhood;
pub mod objects;
pub mod rng;
pub mod romance;
pub mod save;
pub mod school;
pub mod sim;
pub mod social;
pub mod speech;
pub mod starter;
pub mod time;

pub use hearth_ecs::{Entity, World};
pub use sim::{PeonSpec, Sim};
