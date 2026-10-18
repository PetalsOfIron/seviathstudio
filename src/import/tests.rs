// SPDX-License-Identifier: GPL-3.0-or-later
// Tests of the Live2D importer: a small model written by hand, and (when the files are on this computer) the real
// models, which are checked and drawn to target/screens.

use super::caff::tests::make;
use super::cmo3::import_bytes;
use crate::rig::{Rig, Values};

/// A model with one rotation (Angle Z), a warp inside it, and a mesh inside the warp that closes with "Eye open".
pub(crate) fn model_xml() -> String {
    let grid = |bind: &str, keys: &[f32], forms: &[&str], first: bool| -> String {
        let bdef = if first {
            format!("<KeyformBindingSource xs.id=\"{bind}\"><CParameterGuid xs.n=\"parameterGuid\" xs.ref=\"{}\"/><array_list xs.n=\"keys\" count=\"{}\">{}</array_list></KeyformBindingSource>", if bind == "#bz" { "#pz" } else { "#pe" }, keys.len(), keys.iter().map(|k| format!("<f>{k}</f>")).collect::<String>())
        } else {
            format!("<KeyformBindingSource xs.ref=\"{bind}\"/>")
        };
        let on: String = forms.iter().enumerate().map(|(i, f)| format!("<KeyformOnGrid><KeyformGridAccessKey xs.n=\"accessKey\"><array_list xs.n=\"_keyOnParameterList\" count=\"1\"><KeyOnParameter><KeyformBindingSource xs.n=\"binding\" xs.ref=\"{bind}\"/><i xs.n=\"keyIndex\">{i}</i></KeyOnParameter></array_list></KeyformGridAccessKey><CFormGuid xs.n=\"keyformGuid\" xs.ref=\"{f}\"/></KeyformOnGrid>")).collect();
        format!("<KeyformGridSource xs.n=\"keyformGridSource\"><array_list xs.n=\"keyformsOnGrid\" count=\"{}\">{on}</array_list><array_list xs.n=\"keyformBindings\" count=\"1\">{bdef}</array_list></KeyformGridSource>", forms.len())
    };
    let form_head = |g: &str| format!("<ACDeformerForm xs.n=\"super\"><ACForm xs.n=\"super\"><CFormGuid xs.n=\"guid\" xs.id=\"{g}\"/></ACForm></ACDeformerForm>");
    let mesh_head = |g: &str, order: i32| format!("<ACDrawableForm xs.n=\"super\"><ACForm xs.n=\"super\"><CFormGuid xs.n=\"guid\" xs.id=\"{g}\"/></ACForm><i xs.n=\"drawOrder\">{order}</i><f xs.n=\"opacity\">1.0</f></ACDrawableForm>");
    // the warp: 2 by 2 cells, in the rotation's pixels around its origin (200, 200): -50..50
    let unit = |dx: f32| -> String { (0..3).flat_map(|r| (0..3).map(move |c| format!("{} {}", -50.0 + 50.0 * c as f32 + dx, -50.0 + 50.0 * r as f32))).collect::<Vec<_>>().join(" ") };
    format!(
        "<?xml version=\"1.0\"?><CModelSource><s xs.n=\"name\">Test model</s><CImageCanvas xs.n=\"canvas\"><i xs.n=\"pixelWidth\">400</i><i xs.n=\"pixelHeight\">400</i></CImageCanvas>\
<CParameterSourceSet xs.n=\"parameterSourceSet\"><carray_list xs.n=\"_sources\" count=\"2\">\
<CParameterSource><CParameterGuid xs.n=\"guid\" xs.id=\"#pz\"/><f xs.n=\"minValue\">-30.0</f><f xs.n=\"maxValue\">30.0</f><f xs.n=\"defaultValue\">0.0</f><s xs.n=\"name\">Angle Z</s></CParameterSource>\
<CParameterSource><CParameterGuid xs.n=\"guid\" xs.id=\"#pe\"/><f xs.n=\"minValue\">0.0</f><f xs.n=\"maxValue\">1.0</f><f xs.n=\"defaultValue\">1.0</f><s xs.n=\"name\">Eye open</s></CParameterSource>\
</carray_list></CParameterSourceSet>\
<CRotationDeformerSource xs.id=\"#rot\"><ACDeformerSource xs.n=\"super\"><ACParameterControllableSource xs.n=\"super\"><s xs.n=\"localName\">Head turn</s>{rg}</ACParameterControllableSource><CDeformerGuid xs.n=\"guid\" xs.id=\"#dr\"/><CDeformerGuid xs.n=\"targetDeformerGuid\" xs.ref=\"#root\"/></ACDeformerSource>\
<carray_list xs.n=\"keyforms\" count=\"3\">\
<CRotationDeformerForm angle=\"-20.0\" originX=\"200.0\" originY=\"200.0\" scale=\"1.0\">{f1}</CRotationDeformerForm>\
<CRotationDeformerForm angle=\"0.0\" originX=\"200.0\" originY=\"200.0\" scale=\"1.0\">{f2}</CRotationDeformerForm>\
<CRotationDeformerForm angle=\"20.0\" originX=\"200.0\" originY=\"200.0\" scale=\"1.0\">{f3}</CRotationDeformerForm></carray_list></CRotationDeformerSource>\
<CWarpDeformerSource xs.id=\"#warp\"><ACDeformerSource xs.n=\"super\"><ACParameterControllableSource xs.n=\"super\"><s xs.n=\"localName\">Face warp</s>{wg}</ACParameterControllableSource><CDeformerGuid xs.n=\"guid\" xs.id=\"#dw\"/><CDeformerGuid xs.n=\"targetDeformerGuid\" xs.ref=\"#dr\"/></ACDeformerSource>\
<i xs.n=\"col\">2</i><i xs.n=\"row\">2</i>\
<carray_list xs.n=\"keyforms\" count=\"1\"><CWarpDeformerForm>{fw}<float-array xs.n=\"positions\" count=\"18\">{wpos}</float-array></CWarpDeformerForm></carray_list></CWarpDeformerSource>\
<CArtMeshSource xs.id=\"#mesh\"><ACDrawableSource xs.n=\"super\"><ACParameterControllableSource xs.n=\"super\"><s xs.n=\"localName\">Eye</s><b xs.n=\"isVisible\">true</b>{mg}</ACParameterControllableSource><CDrawableGuid xs.n=\"guid\" xs.id=\"#dm\"/><CDeformerGuid xs.n=\"targetDeformerGuid\" xs.ref=\"#dw\"/><carray_list xs.n=\"clipGuidList\" count=\"0\"/></ACDrawableSource>\
<int-array xs.n=\"indices\" count=\"6\">0 1 2 0 2 3</int-array>\
<carray_list xs.n=\"keyforms\" count=\"2\">\
<CArtMeshForm>{m1}<float-array xs.n=\"positions\" count=\"8\">0.25 0.5 0.75 0.5 0.75 0.5 0.25 0.5</float-array></CArtMeshForm>\
<CArtMeshForm>{m2}<float-array xs.n=\"positions\" count=\"8\">0.25 0.4 0.75 0.4 0.75 0.6 0.25 0.6</float-array></CArtMeshForm></carray_list>\
<float-array xs.n=\"positions\" count=\"8\">0 0 1 0 1 1 0 1</float-array>\
<float-array xs.n=\"uvs\" count=\"8\">0 0 1 0 1 1 0 1</float-array>\
<GTexture2D xs.n=\"texture\" xs.id=\"#tex\"><CImageResource xs.n=\"srcImageResource\" width=\"2\" height=\"2\" xs.id=\"#img\"><file xs.n=\"imageFileBuf\" path=\"tex.png\"/></CImageResource></GTexture2D></CArtMeshSource>\
</CModelSource>",
        rg = grid("#bz", &[-30.0, 0.0, 30.0], &["#f1", "#f2", "#f3"], true),
        f1 = form_head("#f1"),
        f2 = form_head("#f2"),
        f3 = form_head("#f3"),
        wg = "<KeyformGridSource xs.n=\"keyformGridSource\"><array_list xs.n=\"keyformsOnGrid\" count=\"1\"><KeyformOnGrid><KeyformGridAccessKey xs.n=\"accessKey\"><array_list xs.n=\"_keyOnParameterList\" count=\"0\"/></KeyformGridAccessKey><CFormGuid xs.n=\"keyformGuid\" xs.ref=\"#fw\"/></KeyformOnGrid></array_list><array_list xs.n=\"keyformBindings\" count=\"0\"/></KeyformGridSource>",
        fw = form_head("#fw"),
        wpos = unit(0.0),
        mg = grid("#be", &[0.0, 1.0], &["#m1", "#m2"], true),
        m1 = mesh_head("#m1", 500),
        m2 = mesh_head("#m2", 500),
    )
}

pub(crate) fn texture_png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(2, 2, |x, y| if (x + y) % 2 == 0 { image::Rgba([200, 30, 30, 255]) } else { image::Rgba([30, 30, 200, 255]) });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn a_hand_made_model_becomes_a_rig_with_the_same_motion() {
    let xml = model_xml();
    let png = texture_png();
    let data = make(0x7273423d, &[("tex.png", &png, false), ("main.xml", xml.as_bytes(), true)]);
    let imp = import_bytes(data).unwrap();
    assert_eq!(imp.name, "Test model");
    assert_eq!((imp.doc.w, imp.doc.h), (400, 400));
    let rig = &*imp.doc.rig;
    let id_of = |n: &str| rig.params.iter().find(|p| p.name == n).map(|p| p.id).unwrap();
    assert_eq!(imp.report.params, 2);
    assert_eq!((imp.report.rotations, imp.report.warps, imp.report.morphs, imp.report.meshes), (1, 1, 1, 1));
    // the rotation turns about its origin, at the key points
    let rot = rig.changers.iter().find(|c| c.name == "Head turn").unwrap();
    assert!(matches!(rot.kind, crate::rig::Kind::Rotation { pivot } if pivot == [200.0, 200.0]));
    let mut v = Values::default();
    v.set(id_of("Angle Z"), 30.0);
    assert_eq!(rig.pose(&v).rot[&rot.id], 20.0);
    v.set(id_of("Angle Z"), -15.0);
    assert_eq!(rig.pose(&v).rot[&rot.id], -10.0, "half way to the first key");
    // the warp is laid over the box of its default shape, inside the rotation
    let warp = rig.changers.iter().find(|c| c.name == "Face warp").unwrap();
    assert!(matches!(&warp.kind, crate::rig::Kind::Warp { rect, cols: 2, rows: 2 } if *rect == [150.0, 150.0, 250.0, 250.0]));
    assert_eq!(warp.parent, Some(rot.id));
    // the mesh sits where its unit positions say: the picture is painted there
    let m = &rig.meshes[0];
    assert_eq!(m.verts[0], [175.0, 190.0], "0.25, 0.4 of the box 150..250 (open eye form)");
    assert_eq!(imp.doc.layers.len(), 2, "one layer and the paper");
    let layer = &imp.doc.layers[0];
    assert_eq!(layer.name, "Eye");
    assert!(!layer.tiles.is_empty());
    // it moves with the head: the corner turns about (200, 200)
    let before = rig.deformed(m, &rig.pose(&Values::default()))[0];
    let mut z = Values::default();
    z.set(id_of("Angle Z"), 30.0);
    let after = rig.deformed(m, &rig.pose(&z))[0];
    let (s, c) = 20f32.to_radians().sin_cos();
    let (dx, dy) = (before[0] - 200.0, before[1] - 200.0);
    let want = [200.0 + dx * c - dy * s, 200.0 + dx * s + dy * c];
    assert!((after[0] - want[0]).abs() < 0.5 && (after[1] - want[1]).abs() < 0.5, "{after:?} vs {want:?}");
    // closing the eye flattens the mesh to its middle line, like the second shape says
    let mut closed = Values::default();
    closed.set(id_of("Eye open"), 0.0);
    let flat = rig.deformed(m, &rig.pose(&closed));
    assert!(flat.iter().all(|p| (p[1] - 200.0).abs() < 4.0), "{flat:?}");
    // and at the default (open) it is untouched
    let open = rig.deformed(m, &rig.pose(&Values::default()));
    for (a, b) in open.iter().zip(&m.verts) {
        assert!((a[0] - b[0]).abs() < 0.5 && (a[1] - b[1]).abs() < 0.5);
    }
}

#[test]
fn files_that_are_not_models_are_refused_clearly() {
    assert!(import_bytes(b"hello".to_vec()).is_err());
    let data = make(0x11223344, &[("main.xml", b"<?xml version=\"1.0\"?><Other/>", true)]);
    assert_eq!(import_bytes(data).err().as_deref(), Some("The file has no model in it."));
    let data = make(0x11223344, &[("tex.png", b"x", false)]);
    assert!(import_bytes(data).err().unwrap().contains("main.xml"));
    let _ = Rig::default();
}

/// The real models, when they are on this computer: they must open, keep their motion, and draw.
#[test]
fn real_models_open_and_draw() {
    use crate::rig::raster::{self, Item, LayerTex};
    let docs = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Documents");
    let names = ["Emela", "Beija", "Semi", "vtuberscale"];
    let _ = std::fs::create_dir_all("target/screens");
    for n in names {
        let path = docs.join(format!("{n}.cmo3"));
        if !path.exists() {
            continue;
        }
        let t0 = std::time::Instant::now();
        let imp = super::cmo3::import_file(&path).unwrap_or_else(|e| panic!("{n}: {e}"));
        eprintln!("{n}: {} params, {} meshes, {} warps, {} rotations, {} morphs in {:?}", imp.report.params, imp.report.meshes, imp.report.warps, imp.report.rotations, imp.report.morphs, t0.elapsed());
        for note in &imp.report.notes {
            eprintln!("  note: {note}");
        }
        assert!(imp.report.meshes > 10 && imp.report.params > 5, "{n} has parts");
        let (w, h) = (imp.doc.w, imp.doc.h);
        let scale = (900.0 / w.max(h) as f32).min(1.0);
        let (pw, ph) = (((w as f32 * scale) as usize).max(8), ((h as f32 * scale) as usize).max(8));
        let rig = &*imp.doc.rig;
        let render = |vals: &Values, tag: &str| {
            let pose = rig.pose(vals);
            let all = rig.deformed_all(&pose);
            let texes: Vec<_> = imp.doc.layers.iter().rev().filter_map(|l| rig.mesh_of(l.id).map(|m| (m, LayerTex::of(l, w, h)))).collect();
            let items: Vec<Item> = texes.iter().filter_map(|(m, t)| t.as_ref().map(|t| Item { tex: t, rest: &m.verts, now: all[rig.meshes.iter().position(|x| x.id == m.id).unwrap()].clone(), tris: &m.tris, opacity: 1.0 })).collect();
            let px = raster::render(&items, pw, ph, scale, Some(eframe::egui::Color32::from_rgb(244, 242, 236)));
            let mut img = image::RgbaImage::new(pw as u32, ph as u32);
            for (i, p) in px.iter().enumerate() {
                img.put_pixel((i % pw) as u32, (i / pw) as u32, image::Rgba([p.r(), p.g(), p.b(), 255]));
            }
            img.save(format!("target/screens/cmo3_{n}_{tag}.png")).unwrap();
        };
        render(&Values::default(), "rest");
        // every slider to the high end, one at a time for the angle sliders
        let mut v = Values::default();
        for p in rig.params.iter().filter(|p| p.name.starts_with("Angle")) {
            v.set(p.id, p.max * 0.8);
        }
        render(&v, "turned");
    }
}
