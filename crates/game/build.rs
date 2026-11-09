//! Embeds every material in `assets/textures/<id>/` (`albedo.png`, optional `normal.png`,
//! `orm.png`) as `TEXTURES` in `$OUT_DIR/textures.rs`, and every object model
//! `assets/models/objects/<object id>.glb` as `OBJECT_MODELS` in `$OUT_DIR/object_models.rs`.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    // a directory is rescanned recursively for changes
    println!("cargo::rerun-if-changed={}", root.display());
    let mut dirs: Vec<_> = std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    let mut out = String::from("/// (material id, albedo, normal, orm) PNG bytes.\npub type TextureSet = (&'static str, &'static [u8], Option<&'static [u8]>, Option<&'static [u8]>);\npub static TEXTURES: &[TextureSet] = &[\n");
    for dir in dirs {
        let file = |name: &str| Some(dir.join(name)).filter(|p| p.is_file()).map(|p| format!("include_bytes!({:?})", p.canonicalize().unwrap()));
        let opt = |name: &str| file(name).map_or("None".into(), |b| format!("Some({b})"));
        let Some(albedo) = file("albedo.png") else {
            println!("cargo::warning=textures: {} has no albedo.png; skipped", dir.display());
            continue;
        };
        let id = dir.file_name().unwrap().to_string_lossy().into_owned();
        writeln!(out, "    ({id:?}, {albedo}, {}, {}),", opt("normal.png"), opt("orm.png")).unwrap();
    }
    out.push_str("];\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("textures.rs"), out).unwrap();
    object_models();
}

fn object_models() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/objects");
    println!("cargo::rerun-if-changed={}", root.display());
    let mut files: Vec<_> = std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "glb")).collect();
    files.sort();
    let mut out = String::from("/// (object id, .glb bytes).\npub static OBJECT_MODELS: &[(&str, &[u8])] = &[\n");
    for f in files {
        let id = f.file_stem().unwrap().to_string_lossy().into_owned();
        writeln!(out, "    ({id:?}, include_bytes!({:?})),", f.canonicalize().unwrap()).unwrap();
    }
    out.push_str("];\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("object_models.rs"), out).unwrap();
}
