//! Skinned models from binary glTF (`.glb`): a skeleton, skinned meshes with morph targets, and
//! animation clips sampled on the CPU into a [`Pose`].
//!
//! Only what the game's assets use is supported: one skin per file, embedded buffers, and
//! translation/rotation/scale channels (step, linear and cubic-spline samplers).

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3, Vec4};

use crate::{MAX_MORPHS, SkinnedMeshData, SkinnedVertex};

/// A local joint transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trs {
    pub t: Vec3,
    pub r: Quat,
    pub s: Vec3,
}

impl Default for Trs {
    fn default() -> Self {
        Trs { t: Vec3::ZERO, r: Quat::IDENTITY, s: Vec3::ONE }
    }
}

impl Trs {
    pub fn to_mat4(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.s, self.r, self.t)
    }
    pub fn lerp(self, o: Trs, w: f32) -> Trs {
        Trs { t: self.t.lerp(o.t, w), r: self.r.slerp(o.r, w), s: self.s.lerp(o.s, w) }
    }
}

/// Local transforms for every joint of a [`Skeleton`], in skin order.
pub type Pose = Vec<Trs>;

#[derive(Clone, Debug)]
pub struct Joint {
    pub name: String,
    /// Parent joint, if the parent node is part of the skin.
    pub parent: Option<usize>,
    pub rest: Trs,
    pub inverse_bind: Mat4,
    /// World transform of the non-joint nodes above a root joint (identity for non-roots).
    pub base: Mat4,
}

#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    pub joints: Vec<Joint>,
    /// Joint indices with every parent before its children.
    order: Vec<usize>,
}

impl Skeleton {
    pub fn joint(&self, name: &str) -> Option<usize> {
        self.joints.iter().position(|j| j.name == name)
    }

    pub fn rest_pose(&self) -> Pose {
        self.joints.iter().map(|j| j.rest).collect()
    }

    /// Model-space transform of every joint.
    pub fn globals(&self, pose: &[Trs]) -> Vec<Mat4> {
        let mut g = vec![Mat4::IDENTITY; self.joints.len()];
        for &i in &self.order {
            let j = &self.joints[i];
            let parent = j.parent.map_or(j.base, |p| g[p]);
            g[i] = parent * pose[i].to_mat4();
        }
        g
    }

    /// Skinning matrices (joint global × inverse bind), appended to `out`.
    pub fn skin_matrices(&self, pose: &[Trs], out: &mut Vec<Mat4>) {
        let g = self.globals(pose);
        out.extend(g.iter().zip(&self.joints).map(|(g, j)| *g * j.inverse_bind));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prop {
    T,
    R,
    S,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Interp {
    Step,
    Linear,
    Cubic,
}

#[derive(Clone, Debug)]
struct Channel {
    joint: usize,
    prop: Prop,
    interp: Interp,
    times: Vec<f32>,
    /// One value per key (cubic-spline tangents are dropped; keys are sampled densely anyway).
    values: Vec<Vec4>,
}

impl Channel {
    fn sample(&self, t: f32) -> Vec4 {
        let n = self.times.len();
        if n == 1 || t <= self.times[0] {
            return self.values[0];
        }
        if t >= self.times[n - 1] {
            return self.values[n - 1];
        }
        let k = self.times.partition_point(|&x| x <= t).max(1) - 1;
        let (t0, t1) = (self.times[k], self.times[k + 1]);
        let w = if self.interp == Interp::Step { 0.0 } else { ((t - t0) / (t1 - t0).max(1e-6)).clamp(0.0, 1.0) };
        let (a, b) = (self.values[k], self.values[k + 1]);
        if self.prop == Prop::R {
            Quat::from_vec4(a).slerp(Quat::from_vec4(b), w).into()
        } else {
            a.lerp(b, w)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    /// Seconds.
    pub duration: f32,
    channels: Vec<Channel>,
}

impl Clip {
    /// Writes the clip's channels at time `t` (seconds, wrapped to the clip length) into `pose`.
    pub fn sample(&self, t: f32, pose: &mut [Trs]) {
        let t = if self.duration > 0.0 { t.rem_euclid(self.duration) } else { 0.0 };
        for c in &self.channels {
            let v = c.sample(t);
            let j = &mut pose[c.joint];
            match c.prop {
                Prop::T => j.t = v.truncate(),
                Prop::R => j.r = Quat::from_vec4(v).normalize(),
                Prop::S => j.s = v.truncate(),
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModelMesh {
    pub name: String,
    pub data: SkinnedMeshData,
    /// Morph target names (from the mesh's `targetNames` extra), at most [`MAX_MORPHS`].
    pub morph_names: Vec<String>,
}

impl ModelMesh {
    pub fn morph(&self, name: &str) -> Option<usize> {
        self.morph_names.iter().position(|n| n == name)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub skeleton: Skeleton,
    pub meshes: Vec<ModelMesh>,
    pub clips: Vec<Clip>,
}

impl Model {
    pub fn mesh(&self, name: &str) -> Option<&ModelMesh> {
        self.meshes.iter().find(|m| m.name == name)
    }
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.iter().find(|c| c.name == name)
    }

    /// Loads a `.glb` file. `slot_of` maps a material name to a palette slot (0 = untinted); the
    /// material's base colour becomes the vertex colour, converted to the sRGB the shader expects.
    pub fn load_glb(bytes: &[u8], slot_of: &dyn Fn(&str) -> u32) -> anyhow::Result<Model> {
        let gltf = gltf::Gltf::from_slice(bytes)?;
        let blob = gltf.blob.as_deref();
        let buffers = |b: gltf::Buffer| match b.source() {
            gltf::buffer::Source::Bin => blob,
            gltf::buffer::Source::Uri(_) => None,
        };
        let doc = &gltf.document;
        let skin = doc.skins().next().ok_or_else(|| anyhow::anyhow!("glb has no skin"))?;

        // node hierarchy
        let nodes: Vec<gltf::Node> = doc.nodes().collect();
        let mut parent_of = vec![None; nodes.len()];
        for n in &nodes {
            for c in n.children() {
                parent_of[c.index()] = Some(n.index());
            }
        }
        let local = |i: usize| {
            let (t, r, s) = nodes[i].transform().decomposed();
            Trs { t: Vec3::from(t), r: Quat::from_array(r), s: Vec3::from(s) }
        };
        let world = |mut i: usize| {
            let mut m = local(i).to_mat4();
            while let Some(p) = parent_of[i] {
                m = local(p).to_mat4() * m;
                i = p;
            }
            m
        };

        // skeleton
        let joint_nodes: Vec<usize> = skin.joints().map(|n| n.index()).collect();
        let joint_of: HashMap<usize, usize> = joint_nodes.iter().enumerate().map(|(j, &n)| (n, j)).collect();
        let ibms: Vec<Mat4> = match skin.reader(buffers).read_inverse_bind_matrices() {
            Some(it) => it.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            None => vec![Mat4::IDENTITY; joint_nodes.len()],
        };
        let mut joints = Vec::with_capacity(joint_nodes.len());
        for (j, &n) in joint_nodes.iter().enumerate() {
            let parent = parent_of[n].and_then(|p| joint_of.get(&p).copied());
            let base = match (parent, parent_of[n]) {
                (None, Some(p)) => world(p),
                _ => Mat4::IDENTITY,
            };
            joints.push(Joint {
                name: nodes[n].name().unwrap_or("").to_string(),
                parent,
                rest: local(n),
                inverse_bind: ibms.get(j).copied().unwrap_or(Mat4::IDENTITY),
                base,
            });
        }
        let mut order = Vec::with_capacity(joints.len());
        let mut placed = vec![false; joints.len()];
        while order.len() < joints.len() {
            let before = order.len();
            for (i, j) in joints.iter().enumerate() {
                if !placed[i] && j.parent.is_none_or(|p| placed[p]) {
                    placed[i] = true;
                    order.push(i);
                }
            }
            anyhow::ensure!(order.len() > before, "joint hierarchy has a cycle");
        }
        let skeleton = Skeleton { joints, order };

        // meshes
        let mut meshes = Vec::new();
        for node in doc.nodes() {
            let Some(mesh) = node.mesh() else { continue };
            if node.skin().is_none() {
                log::warn!("glb: mesh node {:?} is not skinned; skipped", node.name());
                continue;
            }
            let morph_names: Vec<String> = mesh
                .extras()
                .as_ref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw.get()).ok())
                .and_then(|v| v.get("targetNames").cloned())
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let mut data = SkinnedMeshData::default();
            for prim in mesh.primitives() {
                let r = prim.reader(buffers);
                let (Some(pos), Some(nrm)) = (r.read_positions(), r.read_normals()) else { continue };
                let mat = prim.material();
                let slot = slot_of(mat.name().unwrap_or(""));
                let c = mat.pbr_metallic_roughness().base_color_factor();
                let to_srgb = |x: f32| x.max(0.0).powf(1.0 / 2.2);
                let color = [to_srgb(c[0]), to_srgb(c[1]), to_srgb(c[2]), c[3]];
                let base = data.vertices.len() as u32;
                let joints: Vec<[u16; 4]> = r.read_joints(0).map(|j| j.into_u16().collect()).unwrap_or_default();
                let weights: Vec<[f32; 4]> = r.read_weights(0).map(|w| w.into_f32().collect()).unwrap_or_default();
                let targets: Vec<Vec<[f32; 3]>> =
                    r.read_morph_targets().take(MAX_MORPHS).map(|(p, _, _)| p.map(|p| p.collect()).unwrap_or_default()).collect();
                for (i, (p, n)) in pos.zip(nrm).enumerate() {
                    let mut morph = [[0.0; 3]; MAX_MORPHS];
                    for (k, t) in targets.iter().enumerate() {
                        morph[k] = t.get(i).copied().unwrap_or_default();
                    }
                    data.vertices.push(SkinnedVertex {
                        pos: p,
                        normal: n,
                        color,
                        joints: joints.get(i).copied().unwrap_or_default(),
                        weights: weights.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 0.0]),
                        morph,
                        slot,
                    });
                }
                match r.read_indices() {
                    Some(ix) => data.indices.extend(ix.into_u32().map(|i| i + base)),
                    None => data.indices.extend(base..data.vertices.len() as u32),
                }
            }
            meshes.push(ModelMesh { name: node.name().or(mesh.name()).unwrap_or("").to_string(), data, morph_names });
        }

        // clips
        let mut clips = Vec::new();
        for anim in doc.animations() {
            let mut channels = Vec::new();
            let mut duration = 0.0f32;
            for ch in anim.channels() {
                let Some(&joint) = joint_of.get(&ch.target().node().index()) else { continue };
                let r = ch.reader(buffers);
                let (Some(inputs), Some(outputs)) = (r.read_inputs(), r.read_outputs()) else { continue };
                let times: Vec<f32> = inputs.collect();
                use gltf::animation::util::ReadOutputs;
                let (prop, mut values): (Prop, Vec<Vec4>) = match outputs {
                    ReadOutputs::Translations(it) => (Prop::T, it.map(|v| Vec3::from(v).extend(0.0)).collect()),
                    ReadOutputs::Rotations(it) => (Prop::R, it.into_f32().map(Vec4::from).collect()),
                    ReadOutputs::Scales(it) => (Prop::S, it.map(|v| Vec3::from(v).extend(0.0)).collect()),
                    ReadOutputs::MorphTargetWeights(_) => continue,
                };
                let interp = match ch.sampler().interpolation() {
                    gltf::animation::Interpolation::Step => Interp::Step,
                    gltf::animation::Interpolation::Linear => Interp::Linear,
                    gltf::animation::Interpolation::CubicSpline => Interp::Cubic,
                };
                if interp == Interp::Cubic {
                    // (in-tangent, value, out-tangent) triplets: keep the values
                    values = values.chunks(3).filter_map(|c| c.get(1).copied()).collect();
                }
                if times.is_empty() || values.len() != times.len() {
                    continue;
                }
                duration = duration.max(*times.last().unwrap());
                channels.push(Channel { joint, prop, interp, times, values });
            }
            clips.push(Clip { name: anim.name().unwrap_or("").to_string(), duration, channels });
        }

        Ok(Model { skeleton, meshes, clips })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adult() -> Model {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/peon_adult.glb");
        let bytes = std::fs::read(path).expect("assets/models/peon_adult.glb (run assets/blender/make_peons.py)");
        Model::load_glb(&bytes, &|m| if m == "skin" { 1 } else { 0 }).unwrap()
    }

    #[test]
    fn loads_skeleton_meshes_and_clips() {
        let m = adult();
        assert_eq!(m.skeleton.joints.len(), 17);
        assert!(m.skeleton.joint("head").is_some());
        let body = m.mesh("body").expect("body mesh");
        assert_eq!(body.morph_names, ["slim", "heavy"]);
        assert!(body.data.vertices.iter().any(|v| v.slot == 1));
        for n in ["hair0", "hair1", "hair2", "hair3"] {
            assert!(m.mesh(n).is_some(), "{n}");
        }
        for c in ["idle", "walk", "sit", "lie", "use", "dance", "exercise"] {
            assert!(m.clip(c).is_some_and(|c| c.duration > 0.0), "{c}");
        }
    }

    #[test]
    fn rest_pose_skins_to_identity() {
        let m = adult();
        let mut mats = Vec::new();
        m.skeleton.skin_matrices(&m.skeleton.rest_pose(), &mut mats);
        for (i, s) in mats.iter().enumerate() {
            assert!(s.abs_diff_eq(Mat4::IDENTITY, 1e-4), "joint {} not identity: {s:?}", m.skeleton.joints[i].name);
        }
    }

    #[test]
    fn sit_lowers_the_hips() {
        let m = adult();
        let hips = m.skeleton.joint("hips").unwrap();
        let rest = m.skeleton.globals(&m.skeleton.rest_pose())[hips].w_axis.y;
        let mut pose = m.skeleton.rest_pose();
        m.clip("sit").unwrap().sample(0.0, &mut pose);
        let sat = m.skeleton.globals(&pose)[hips].w_axis.y;
        assert!(rest > 0.7 && sat < 0.2, "hips rest {rest}, sitting {sat}");
    }
}
