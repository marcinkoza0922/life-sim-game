# Material textures

One folder per material id from `assets/data/materials.ron`:

```
<id>/albedo.png   sRGB base colour (required)
<id>/normal.png   tangent-space normal map, OpenGL convention (green = up the image); optional
<id>/orm.png      linear R = ambient occlusion, G = roughness, B = metallic; optional
```

Any square size works; the renderer resamples every layer to 512×512 with mips. Textures
must tile seamlessly. They are box-projected (triplanar) in each draw's scaled object space, so
no UVs are involved, and one repeat covers the material's `scale` meters. On walls, the top of
the image is up.

`crates/game/build.rs` embeds every folder here into the binary; an id without a folder renders
flat.

Every material here is generated (`floor_*`, `wall_*`, and the tintable `wood`, `fabric`, `metal` and `stone`,
which stay near-neutral because parts multiply them by their colour): `python3 assets/materialcraft/materials.py
[id ...]` rebuilds `assets/materialcraft/<id>.sbs` and these maps headlessly with
`materialcraft-cli` (edit the expressions there rather than the PNGs). Check the results with
`simctl materials --out out/materials.png` (one swatch per material) or a `simctl render`.
