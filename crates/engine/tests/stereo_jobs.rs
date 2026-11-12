use anicraft_doc::{Document, Element, ElementId, ElementKind, Keyframe, Layer, LayerId, Paint, Rgba, Shape, StereoStage};
use anicraft_engine::render_service::{RenderService, make_job, make_stereo_jobs, render_batch};
use anicraft_geom::{Affine, Path};
use anicraft_render::View;
use std::sync::Arc;

fn document(depth: f32) -> Document {
    let mut doc = Document::default();
    doc.stage.width = 80;
    doc.stage.height = 48;
    let mut layer = Layer::new(LayerId(1), "art");
    layer.depth = depth;
    let mut key = Keyframe::new(0, 20);
    key.elements.push(Arc::new(Element {
        id: ElementId(1),
        name: String::new(),
        color: Default::default(),
        transform: Affine::translate(30.0, 16.0),
        kind: ElementKind::Shape(Shape::filled(Path::rect(0.0, 0.0, 12.0, 12.0), Paint::Solid(Rgba::BLACK))),
    }));
    layer.keyframes = vec![Arc::new(key)];
    doc.timeline_mut(0).layers = vec![Arc::new(layer)];
    doc
}
fn view(doc: &Document) -> View {
    View { transform: Affine::IDENTITY, width: doc.stage.width, height: doc.stage.height, background: Some(doc.stage.background) }
}
#[test]
fn cached_batch_keeps_both_eyes_and_one_immutable_timeline_frame() {
    let doc = document(1000.0);
    let before = serde_json::to_value(&doc).unwrap();
    let stereo = StereoStage::new(12.0, 4.0).unwrap();
    let jobs = make_stereo_jobs(17, &doc, 7, view(&doc), stereo);
    assert_ne!(jobs[0].key, jobs[1].key, "image cache cannot reuse the left eye");
    assert_ne!(jobs[0].scene, jobs[1].scene, "display-list cache cannot reuse the left eye");
    let images = render_batch(&jobs);
    assert_ne!(images[0].rgba_premul, images[1].rgba_premul);
    assert!(images.iter().all(|i| i.frame == 7 && i.width == 80 && i.height == 48 && i.error.is_none()));
    let service = RenderService::new(1 << 20, None);
    let queued = service.request_batch(jobs.to_vec());
    for (job, direct) in jobs.iter().zip(&images) {
        let cached = service.wait_for(job.key, 10_000).unwrap();
        assert_eq!(cached.rgba_premul, direct.rgba_premul);
    }
    assert_eq!(queued.len(), 2);
    let other_budget = make_stereo_jobs(17, &doc, 7, view(&doc), StereoStage::new(6.0, 2.0).unwrap());
    let other_frame = make_stereo_jobs(17, &doc, 8, view(&doc), stereo);
    for eye in 0..2 {
        assert_ne!(jobs[eye].key, other_budget[eye].key);
        assert_ne!(jobs[eye].scene, other_budget[eye].scene);
        assert_ne!(jobs[eye].key, other_frame[eye].key);
    }
    assert_eq!(before, serde_json::to_value(&doc).unwrap());
}
#[test]
fn flat_art_has_the_exact_ordinary_camera_pixels_in_both_eyes() {
    let doc = document(0.0);
    let mono = make_job(17, &doc, 7, view(&doc), None).through_camera(true);
    let pair = make_stereo_jobs(17, &doc, 7, view(&doc), StereoStage::new(12.0, 4.0).unwrap());
    let images = render_batch(&[mono, pair[0].clone(), pair[1].clone()]);
    assert_eq!(images[0].rgba_premul, images[1].rgba_premul);
    assert_eq!(images[0].rgba_premul, images[2].rgba_premul);
}

#[test]
fn interactive_pair_holds_state_when_the_player_moves_or_changes_without_a_tick() {
    use anicraft_engine::render_service::make_player_stereo_jobs;
    use anicraft_doc::player::{PlayerState, Overrides};
    let mut doc = document(0.0);
    let mut inner = (*doc.timeline(0).layers[0]).clone();
    Arc::make_mut(&mut Arc::make_mut(&mut inner.keyframes[0]).elements[0]).transform = Affine::IDENTITY;
    let symbol = anicraft_doc::SymbolId(800);
    doc.library.push(Arc::new(anicraft_doc::Symbol { id: symbol, name: "Actor".into(), kind: anicraft_doc::SymbolKind::Graphic, linkage: None, scale_grid: None, scale_grid_kept: None, timeline: anicraft_doc::Timeline { layers: vec![Arc::new(inner)] } }));
    Arc::make_mut(&mut Arc::make_mut(&mut Arc::make_mut(&mut doc.timeline_mut(0).layers[0]).keyframes[0]).elements[0]).kind = ElementKind::Symbol { symbol, loop_mode: Default::default(), first_frame: 0, blend: Default::default(), filters: Vec::new(), events: Vec::new() };
    let mut state = PlayerState::default();
    state.root.frame = 7;
    let first = make_player_stereo_jobs(17, &doc, &state, 1, view(&doc), StereoStage::new(12.0, 4.0).unwrap());
    state.root.frame = 8;
    state.props.insert(vec![ElementId(1)], Overrides { x: Some(40.0), ..Default::default() });
    let next = make_player_stereo_jobs(17, &doc, &state, 2, view(&doc), StereoStage::new(12.0, 4.0).unwrap());
    assert!(first.iter().all(|job| job.frame == 7));
    assert!(next.iter().all(|job| job.frame == 8));
    assert_ne!(first[0].key, next[0].key);
    assert_ne!(first[0].scene, next[0].scene);
    let a = render_batch(&first);
    let b = render_batch(&next);
    assert_eq!(a[0].rgba_premul, a[1].rgba_premul);
    assert_eq!(b[0].rgba_premul, b[1].rgba_premul);
    assert_ne!(a[0].rgba_premul, b[0].rgba_premul);
    state.props.get_mut(&[ElementId(1)][..]).unwrap().x = Some(45.0);
    let input = make_player_stereo_jobs(17, &doc, &state, 3, view(&doc), StereoStage::new(12.0, 4.0).unwrap());
    assert_eq!(input[0].frame, next[0].frame);
    assert_ne!(input[0].key, next[0].key);
    assert_ne!(input[0].scene, next[0].scene);
}
