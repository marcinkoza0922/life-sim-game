//! Renders a small panel on a headless GPU: images (rounded, outlined, in a button) and a text
//! field with a selection. Checks a few pixels and writes `out/hearth_ui_snapshot.png`.

use hearth_render::wgpu;
use hearth_ui::{Button, Order, Paint, Rect, Ui, theme, vec2};

const W: u32 = 360;
const H: u32 = 220;

/// 16x16: left half red, right half blue.
fn split_image() -> Vec<u8> {
    (0..16 * 16).flat_map(|i| if i % 16 < 8 { [230, 40, 40, 255] } else { [40, 60, 230, 255] }).collect()
}

fn build(ui: &mut Ui, name: &mut String, img: hearth_ui::ImageId) {
    ui.begin_frame(1.0 / 60.0, [W, H]);
    ui.painter.rect(Rect::new(0.0, 0.0, W as f32, H as f32), 0.0, Paint::fill(theme::CREAM));
    ui.area("panel", Order::Panel, vec2(20.0, 20.0), 320.0, |ui| {
        ui.row(100.0, |ui| {
            ui.image_framed(img, vec2(100.0, 100.0), 30.0);
            ui.image(img, vec2(40.0, 40.0));
            ui.btn(Button::new("Buy").icon(img));
        });
        ui.space(10.0);
        ui.text_field("name", name, "Name", 240.0);
    });
    ui.end_frame();
}

#[test]
fn draws_images_and_selection() {
    let Ok(gpu) = hearth_render::Gpu::headless() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let host = hearth_render::Renderer::new(gpu.clone(), format);
    let mut renderer = hearth_ui::Renderer::new(&gpu.device, format);
    let mut ui = Ui::new();
    ui.instant = true;
    let img = ui.images.add_rgba(16, 16, &split_image()).unwrap();
    let mut name = String::from("Bramble Hearthstone");

    // focus the field (it sits below the 100pt row), then select the surname
    build(&mut ui, &mut name, img);
    ui.input.pointer = Some(vec2(40.0, 150.0));
    (ui.input.down[0], ui.input.pressed[0]) = (true, true);
    build(&mut ui, &mut name, img);
    (ui.input.down[0], ui.input.released[0]) = (false, true);
    ui.input.keys.push(hearth_ui::Key::End);
    build(&mut ui, &mut name, img);
    ui.input.mods.shift = true;
    ui.input.mods.command = true;
    ui.input.keys.push(hearth_ui::Key::Left);
    build(&mut ui, &mut name, img);
    assert_eq!(&name[ui.anchor.min(ui.caret)..ui.anchor.max(ui.caret)], "Hearthstone");

    renderer.prepare(&gpu.device, &gpu.queue, &mut ui, [W, H]);
    let tex = host.offscreen_texture(W, H);
    let view = tex.create_view(&Default::default());
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui test"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        renderer.paint(&mut pass);
    }
    gpu.queue.submit([enc.finish()]);
    let px = host.readback(&tex, W, H);
    let at = |x: u32, y: u32| -> [u8; 3] {
        let i = ((y * W + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    std::fs::create_dir_all("../../out").ok();
    image::RgbaImage::from_raw(W, H, px.clone()).unwrap().save("../../out/hearth_ui_snapshot.png").unwrap();

    // framed image at (20,20)-(120,120): red left, blue right, rounded corner shows the cream
    let [r, _, b] = at(45, 70);
    assert!(r > 200 && b < 80, "left half red: {:?}", at(45, 70));
    let [r, _, b] = at(95, 70);
    assert!(b > 200 && r < 80, "right half blue: {:?}", at(95, 70));
    let cream = at(5, 5);
    assert_eq!(at(22, 22), cream, "corner cut by the 30pt radius");
    let ink = at(20, 70);
    assert!(ink.iter().all(|&c| c < 120), "outline: {ink:?}");
}
