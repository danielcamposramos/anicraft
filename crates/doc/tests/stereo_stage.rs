use anicraft_doc::{
    Document, Element, ElementId, ElementKind, Keyframe, Layer, LayerId, Paint, Rgba, Shape, evaluate_scene_camera, evaluate_scene_stereo,
};
use anicraft_geom::{Affine, Path, Vec2};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::default();
    doc.stage.width = 320;
    doc.stage.height = 180;
    doc.timeline_mut(0).layers = [-250.0, 0.0, 1000.0]
        .into_iter()
        .enumerate()
        .map(|(i, depth)| {
            let mut layer = Layer::new(LayerId(i as u64 + 1), format!("depth{depth}"));
            layer.depth = depth;
            let mut key = Keyframe::new(0, 20);
            key.elements.push(Arc::new(Element {
                id: ElementId(i as u64 + 10),
                name: String::new(),
                color: Default::default(),
                transform: Affine::translate(40.0 + i as f32 * 70.0, 60.0),
                kind: ElementKind::Shape(Shape::filled(Path::rect(0.0, 0.0, 12.0, 12.0), Paint::Solid(Rgba::BLACK))),
            }));
            layer.keyframes = vec![Arc::new(key)];
            Arc::new(layer)
        })
        .collect();
    doc
}

#[test]
fn each_layer_has_its_predicted_horizontal_disparity_and_the_same_identity() {
    let doc = document();
    let before = serde_json::to_value(&doc).unwrap();
    let pair = evaluate_scene_stereo(&doc, 0, 7, 12.0, 4.0).unwrap();
    let mono = evaluate_scene_camera(&doc, 0, 7);
    assert_eq!(pair[0].items.len(), 3);
    assert_eq!(pair[1].items.len(), 3);
    for ((left, right), ordinary) in pair[0].items.iter().zip(&pair[1].items).zip(&mono.items) {
        assert_eq!((left.layer, left.element), (right.layer, right.element));
        let depth = doc.timeline(0).layer(left.layer).unwrap().depth;
        let expected = (12.0_f64 * (1000.0 / (1000.0 + f64::from(depth)) - 1.0)).min(4.0);
        let l = left.transform.apply(Vec2::new(0.0, 0.0));
        let r = right.transform.apply(Vec2::new(0.0, 0.0));
        let m = ordinary.transform.apply(Vec2::new(0.0, 0.0));
        assert!((f64::from(l.x - r.x) - expected).abs() < 1e-4, "layer{}: {l:?} {r:?}, expected{expected}", left.layer.0);
        assert_eq!(l.y, r.y);
        assert!(((l.x + r.x) * 0.5 - m.x).abs() < 1e-4);
    }
    assert_eq!(before, serde_json::to_value(&doc).unwrap(), "evaluating either eye must not advance or edit the document");
}

#[test]
fn flat_and_zero_limit_views_are_exactly_the_ordinary_evaluator() {
    let mut doc = document();
    for layer in &mut doc.timeline_mut(0).layers {
        Arc::make_mut(layer).depth = 0.0;
    }
    for frame in [0, 7, 19] {
        let pair = evaluate_scene_stereo(&doc, 0, frame, 12.0, 4.0).unwrap();
        let mono = evaluate_scene_camera(&doc, 0, frame);
        assert_eq!(format!("{:?}", pair[0]), format!("{mono:?}"));
        assert_eq!(format!("{:?}", pair[1]), format!("{mono:?}"));
    }
    let doc = document();
    let pair = evaluate_scene_stereo(&doc, 0, 7, 0.0, 0.0).unwrap();
    assert_eq!(format!("{:?}", pair[0]), format!("{:?}", evaluate_scene_camera(&doc, 0, 7)));
    assert_eq!(format!("{:?}", pair[1]), format!("{:?}", pair[0]));
    for limits in [(f32::NAN, 4.0), (12.0, f32::INFINITY), (-1.0, 4.0), (12.0, -1.0)] {
        assert!(evaluate_scene_stereo(&doc, 0, 7, limits.0, limits.1).is_none());
    }
}

#[test]
fn camera_rotation_does_not_turn_horizontal_stereo_into_vertical_disparity() {
    let mut doc = document();
    let camera =
        anicraft_doc::camera::Camera { zoom: 1.75, rotation: 0.75, center: Vec2::new(120.0, 70.0), ..anicraft_doc::camera::Camera::rest(&doc.stage) };
    let mut layer = Layer::new(LayerId(900), "Camera");
    layer.kind = anicraft_doc::LayerKind::Camera;
    let mut key = Keyframe::new(0, 20);
    key.elements.push(Arc::new(camera.to_element(ElementId(901))));
    layer.keyframes = vec![Arc::new(key)];
    doc.timeline_mut(0).layers.insert(0, Arc::new(layer));
    let pair = evaluate_scene_stereo(&doc, 0, 7, 12.0, 4.0).unwrap();
    for (left, right) in pair[0].items.iter().zip(&pair[1].items) {
        for point in [Vec2::new(0.0, 0.0), Vec2::new(6.0, 8.0)] {
            assert_eq!(left.transform.apply(point).y, right.transform.apply(point).y);
        }
        assert_eq!(&left.transform.m[..4], &right.transform.m[..4], "the existing scale and rotation are unchanged");
    }
}

#[test]
fn artwork_inside_a_symbol_inherits_its_top_level_depth_once() {
    use anicraft_doc::{Symbol, SymbolId, SymbolKind, Timeline};
    let mut doc = document();
    let mut inner = (*doc.timeline(0).layers[0]).clone();
    inner.depth = 10_000.0;
    let symbol = SymbolId(800);
    doc.library.push(Arc::new(Symbol {
        id: symbol,
        name: "Inside".into(),
        kind: SymbolKind::Graphic,
        linkage: None,
        scale_grid: None,
        scale_grid_kept: None,
        timeline: Timeline { layers: vec![Arc::new(inner)] },
    }));
    let tl = doc.timeline_mut(0);
    tl.layers.truncate(1);
    let layer = Arc::make_mut(&mut tl.layers[0]);
    let key = Arc::make_mut(&mut layer.keyframes[0]);
    Arc::make_mut(&mut key.elements[0]).kind = ElementKind::Symbol {
        symbol,
        loop_mode: Default::default(),
        first_frame: 0,
        blend: Default::default(),
        filters: Vec::new(),
        events: Vec::new(),
    };
    let pair = evaluate_scene_stereo(&doc, 0, 7, 12.0, 4.0).unwrap();
    assert_eq!(pair[0].items.len(), 1);
    let left = pair[0].items[0].transform.translation();
    let right = pair[1].items[0].transform.translation();
    assert!((left.x - right.x - 4.0).abs() < 1e-4, "the nested layer's depth must not be applied again");
    assert_eq!(left.y, right.y);
}

#[test]
fn two_views_keep_the_requested_timeline_frame_and_do_not_mutate_it() {
    let mut doc = document();
    let layer = Arc::make_mut(&mut doc.timeline_mut(0).layers[1]);
    Arc::make_mut(&mut layer.keyframes[0]).duration = 10;
    let mut second = (*layer.keyframes[0]).clone();
    second.start = 10;
    Arc::make_mut(&mut second.elements[0]).transform = Affine::translate(160.0, 60.0);
    layer.keyframes.push(Arc::new(second));
    let before = serde_json::to_value(&doc).unwrap();
    for (frame, expected) in [(2, 110.0), (12, 160.0)] {
        let pair = evaluate_scene_stereo(&doc, 0, frame, 12.0, 4.0).unwrap();
        for eye in &pair {
            let item = eye.items.iter().find(|i| i.layer == LayerId(2)).unwrap();
            assert_eq!(item.transform.translation().x, expected);
        }
    }
    assert_eq!(before, serde_json::to_value(&doc).unwrap());
}

#[test]
fn visible_masks_keep_their_clip_identity_and_hidden_layers_stay_hidden() {
    let mut doc = document();
    let tl = doc.timeline_mut(0);
    Arc::make_mut(&mut tl.layers[0]).kind = anicraft_doc::LayerKind::Mask;
    let art = Arc::make_mut(&mut tl.layers[1]);
    art.parent = Some(LayerId(1));
    art.depth = -250.0;
    Arc::make_mut(&mut Arc::make_mut(&mut art.keyframes[0]).elements[0]).transform = Affine::translate(40.0, 60.0);
    Arc::make_mut(&mut tl.layers[2]).visible = false;
    let mono = evaluate_scene_camera(&doc, 0, 7);
    let pair = evaluate_scene_stereo(&doc, 0, 7, 12.0, 4.0).unwrap();
    for eye in &pair {
        assert_eq!(eye.clip_parents, mono.clip_parents);
        assert_eq!(eye.groups, mono.groups);
        assert_eq!(eye.items.len(), 2);
        assert!(eye.items.iter().all(|item| item.layer != LayerId(3)));
        for (item, ordinary) in eye.items.iter().zip(&mono.items) {
            assert_eq!((item.clip, item.masks, item.guide, item.group), (ordinary.clip, ordinary.masks, ordinary.guide, ordinary.group));
        }
        let artwork = eye.items.iter().find(|item| item.layer == LayerId(2)).unwrap();
        let center = artwork.transform.apply(Vec2::new(6.0, 6.0));
        assert!(eye.clip_contains(artwork.clip.unwrap(), center));
    }
}
