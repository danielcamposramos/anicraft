//! A movie-stage view: one immutable frame, two native-resolution depth-plane renders.
//! Desktop protocol and presentation belong to craft-stereo-window; the editor stays ordinary.

use anicraft_doc::StereoStage;
use anicraft_engine::Session;
use anicraft_engine::render_service::{FrameImage, FrameJob, RenderService, make_stereo_jobs};
use anicraft_geom::Affine;
use anicraft_render::{MAX_PIXELS, View};
use craft_stereo_window::{Picture, StereoWindow};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct StageView {
    pub enabled: bool,
    window: Option<StereoWindow>,
    render: Option<RenderService>,
    presented: Option<[u64; 2]>,
    pending: Option<[FrameJob; 2]>,
    warned: bool,
    opened: Option<Instant>,
}

impl StageView {
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.close();
        }
    }

    fn close(&mut self) {
        self.enabled = false;
        self.window = None;
        // Keep the bounded worker until the app drops; closing a view must not join an
        // in-flight CPU render on the UI thread. Reopening can reuse its cached pictures.
        self.pending = None;
        self.presented = None;
        self.warned = false;
        self.opened = None;
    }

    /// Poll feedback and render only a complete pair. Opening/closing and errors never edit
    /// the document or advance its timeline. The renderer's existing bounded worker owns work.
    pub fn tick(&mut self, session: &Session, ctx: &egui::Context) -> Option<String> {
        if !self.enabled {
            return None;
        }
        // Let the editor's initial map settle before opening its optional movie view.
        if ctx.cumulative_frame_nr() < 2 {
            ctx.request_repaint();
            return None;
        }
        match self.update(session, ctx) {
            Ok(message) => message,
            Err(error) => {
                self.close();
                Some(tr!("Could not show the stereo view: {error}", error = error))
            }
        }
    }

    fn update(&mut self, session: &Session, ctx: &egui::Context) -> Result<Option<String>, String> {
        let doc = &session.doc;
        let (w, h) = (doc.stage.width, doc.stage.height);
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
            return Err(tr!("The stereo view supports at most {pixels} stage pixels; reduce the stage dimensions.", pixels = MAX_PIXELS));
        }
        if self.window.is_none() {
            self.opened = Some(Instant::now());
            self.window = Some(crate::platform::open_stereo_window(tr!("Stereo View")).map_err(|e| e.to_string())?);
            if self.render.is_none() {
                let wake = ctx.clone();
                self.render = Some(RenderService::new((MAX_PIXELS * 4 * 4) as usize, Some(Box::new(move || wake.request_repaint()))));
            }
        }
        let Some(window) = self.window.as_ref() else { return Ok(None) };
        if !window.is_open() {
            self.close();
            return Ok(None);
        }
        let limits = window.depth_limits().map(|limits| limits.native_pixels(w));
        let (sunk, popped) = limits.unwrap_or((0.0, 0.0));
        let stereo = StereoStage::new(sunk as f32, popped as f32).ok_or_else(|| tr!("The compositor returned invalid depth limits."))?;
        // The first zero-budget pair maps the native window so the compositor can send its
        // placement and limits. It is the ordinary camera picture, never an invented baseline.
        let frame = movie_frame(session);
        let view = View { transform: Affine::IDENTITY, width: w, height: h, background: Some(doc.stage.background) };
        let jobs = make_stereo_jobs(session.state, doc, frame, view, stereo);
        let keys = [jobs[0].key, jobs[1].key];
        if self.pending.is_none() && self.presented != Some(keys) {
            self.pending = Some(jobs);
        }
        if let Some(pending) = &self.pending {
            let Some(render) = self.render.as_ref() else { return Ok(None) };
            let keys = [pending[0].key, pending[1].key];
            let requested_frame = pending[0].frame;
            let images = render.request_batch(pending.to_vec());
            if let [Some(left), Some(right)] = images.as_slice() {
                let picture = pack(left, right, keys, requested_frame)?;
                if !window.show(picture) {
                    self.close();
                    return Ok(None);
                }
                self.presented = Some(keys);
                self.pending = None;
            }
        }
        // One pending snapshot finishes as a pair even if playback has moved ahead. Then
        // the next tick requests the newest frame: slow rendering drops frames, not eyes.
        // Protocol feedback arrives on the shared window's thread. Poll while the view exists;
        // no accumulating work queue or independent animation clock is introduced.
        ctx.request_repaint_after(Duration::from_millis(16));
        if limits.is_none() && !self.warned && self.opened.is_some_and(|at| at.elapsed() >= Duration::from_secs(3)) {
            self.warned = true;
            return Ok(Some(tr!("Stereo depth is unavailable on this desktop; showing the flat stage.").into()));
        }
        if limits.is_some() {
            self.warned = false;
        }
        Ok(None)
    }
}

/// The movie's playhead, independently of an open symbol's editing playhead.
fn movie_frame(session: &Session) -> u32 {
    session.doc.scene_start(session.scene).saturating_add(session.editing.first().map_or(session.frame, |level| level.frame))
}

/// Copy whole rows, left first. The shared window composites over black: premultiplied RGB
/// bytes are already that opaque result, so setting alpha to 255 avoids a second multiply.
fn pack(left: &FrameImage, right: &FrameImage, keys: [u64; 2], frame: u32) -> Result<Picture, String> {
    if let Some(error) = left.error.as_ref().or(right.error.as_ref()) {
        return Err(error.clone());
    }
    let (w, h) = (left.width as usize, left.height as usize);
    let bytes = w.checked_mul(h).and_then(|n| n.checked_mul(4)).ok_or_else(|| tr!("The stereo views have invalid dimensions."))?;
    if w == 0
        || h == 0
        || w as u64 * h as u64 > MAX_PIXELS
        || right.width != left.width
        || right.height != left.height
        || left.key != keys[0]
        || right.key != keys[1]
        || left.frame != frame
        || right.frame != frame
        || left.rgba_premul.len() != bytes
        || right.rgba_premul.len() != bytes
    {
        return Err(tr!("The stereo views do not match the requested frame and native size.").into());
    }
    let mut rgba = Vec::with_capacity(bytes * 2);
    for (l, r) in left.rgba_premul.chunks_exact(w * 4).zip(right.rgba_premul.chunks_exact(w * 4)) {
        rgba.extend_from_slice(l);
        rgba.extend_from_slice(r);
    }
    for [_, _, _, alpha] in rgba.as_chunks_mut::<4>().0 {
        *alpha = 255;
    }
    Picture::new(w * 2, h, rgba.into(), true).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(key: u64, frame: u32, pixels: Vec<u8>) -> FrameImage {
        FrameImage {
            key,
            frame,
            width: 2,
            height: 2,
            rgba_premul: pixels,
            native: None,
            ms: 0.0,
            stats: Default::default(),
            error: None,
            gpu: false,
            fallback: None,
        }
    }
    #[test]
    fn movie_view_keeps_root_playhead_inside_symbols() {
        use anicraft_doc::SymbolId;
        use anicraft_engine::editing::EditLevel;
        use std::sync::Arc;
        let mut session = Session::default();
        let layer = Arc::make_mut(&mut session.doc.timeline_mut(0).layers[0]);
        Arc::make_mut(&mut layer.keyframes[0]).duration = 20;
        session.frame = 12;
        assert_eq!(movie_frame(&session), 12);
        let outer = EditLevel {
            symbol: SymbolId(10),
            instance: None,
            transform: Affine::IDENTITY,
            layer: session.layer,
            frame: 2,
            selection: Default::default(),
        };
        session.editing.push(outer.clone());
        session.editing.push(EditLevel { symbol: SymbolId(11), frame: 7, ..outer });
        let before = serde_json::to_value(&session.doc).unwrap();
        assert_eq!(movie_frame(&session), 2, "editing a symbol cannot replace the movie's parent playhead");
        assert_eq!(serde_json::to_value(&session.doc).unwrap(), before);
        assert_eq!(session.frame, 12);
        session.editing.clear();
        assert_eq!(movie_frame(&session), 12);
    }

    #[test]
    fn pair_rows_keep_native_eye_order_and_opaque_over_black_pixels() {
        let left = image(1, 7, vec![90, 20, 10, 128, 80, 30, 20, 128, 70, 40, 30, 128, 60, 50, 40, 128]);
        let right = image(2, 7, vec![10, 20, 90, 128, 20, 30, 80, 128, 30, 40, 70, 128, 40, 50, 60, 128]);
        let pair = pack(&left, &right, [1, 2], 7).unwrap();
        assert_eq!((pair.width, pair.height, pair.stereo), (4, 2, true));
        assert_eq!(
            &*pair.rgba,
            &[90, 20, 10, 255, 80, 30, 20, 255, 10, 20, 90, 255, 20, 30, 80, 255, 70, 40, 30, 255, 60, 50, 40, 255, 30, 40, 70, 255, 40, 50, 60, 255]
        );
    }
    #[test]
    fn stale_or_malformed_views_cannot_form_a_pair() {
        let left = image(1, 7, vec![0; 16]);
        let mut right = image(2, 8, vec![0; 16]);
        assert!(pack(&left, &right, [1, 2], 7).is_err());
        right.frame = 7;
        assert!(pack(&left, &right, [1, 9], 7).is_err());
        right.rgba_premul.pop();
        assert!(pack(&left, &right, [1, 2], 7).is_err());
        right.rgba_premul.push(0);
        right.width = 1;
        assert!(pack(&left, &right, [1, 2], 7).is_err());
        right.width = 2;
        right.error = Some("raster failed".into());
        assert!(pack(&left, &right, [1, 2], 7).is_err());
    }
}
