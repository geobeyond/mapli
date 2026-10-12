//! Worker threads for rendering requests. This is where the MapLibre renderers operate.
//!
//! Each worker owns its own renderers as local state of the thread. This is because MapLibre
//! renderers are not `Send`able.

use std::collections::HashMap;
use std::hash::Hash;
use std::num::NonZeroU32;
use std::sync::Arc;

use crossbeam_channel::Receiver;
use image::RgbaImage;
use maplibre_native::{EdgeInsets, ImageRenderer, ImageRendererBuilder, Static, Tile};

use crate::encode::encode;
use crate::error::{MapliError, Result};
use crate::pool::StyleRegistry;
use crate::types::{
    ImageSpec, PixelRatio, Rendered, StaticRequest, Style, StyleId, TileRequest, View,
};

pub(crate) type Reply = oneshot::Sender<Result<Rendered>>;

pub(crate) enum Command {
    Static { req: StaticRequest, reply: Reply },
    Tile { req: TileRequest, reply: Reply },
}

/// Thread entry point. Returns when the channel is disconnected, i.e. when
/// the `RenderPool` has been dropped and the queue is drained.
pub(crate) fn run(rx: Receiver<Command>, styles: StyleRegistry, max_renderers: usize) {
    // this is created on the thread and everything inside `worker` stays in-thread
    let mut worker = Worker::new(styles, max_renderers);

    for cmd in rx.iter() {
        match cmd {
            Command::Static { req, reply } => {
                if reply.is_closed() {
                    continue; // no point in rendering if caller already dropped the receiver
                }
                let format = req.format;
                let result = worker
                    .render_static(req)
                    .and_then(|img| encode(img, format));
                let _ = reply.send(result);
            }
            Command::Tile { req, reply } => {
                if reply.is_closed() {
                    continue;
                }
                let format = req.format;
                let result = worker.render_tile(req).and_then(|img| encode(img, format));
                let _ = reply.send(result);
            }
        }
    }
}

type StaticKey = (StyleId, ImageSpec);
type TileKey = (StyleId, NonZeroU32, PixelRatio);

struct Worker {
    styles: StyleRegistry,
    statics: RendererCache<StaticKey, ImageRenderer<Static>>,
    tiles: RendererCache<TileKey, ImageRenderer<Tile>>,
}

impl Worker {
    fn new(styles: StyleRegistry, max_renderers: usize) -> Self {
        Self {
            styles,
            statics: RendererCache::new(max_renderers),
            tiles: RendererCache::new(max_renderers),
        }
    }

    fn render_static(&mut self, req: StaticRequest) -> Result<RgbaImage> {
        let key = (req.style.clone(), req.spec);

        let styles = &self.styles;
        let renderer = self.statics.get_or_try_insert_with(key, || {
            let style = lookup_style(styles, &req.style)?;
            let mut renderer = ImageRendererBuilder::new()
                .with_size(req.spec.width, req.spec.height)
                .with_pixel_ratio(req.spec.pixel_ratio.get())
                .build_static_renderer();
            load_style(&mut renderer, &req.style, &style)?;
            Ok(renderer)
        })?;

        // Fitting bounds needs the renderer, as the resulting zoom depends on its size
        let camera = match req.view {
            View::Camera(camera) => camera.to_camera_update(),
            View::BoundsToFit(fit) => renderer.camera_for_bounds(
                fit.bounds().to_lat_lng_bounds(),
                Some(EdgeInsets::all(fit.padding())),
                fit.bearing(),
                fit.pitch(),
            ),
        };
        let image = renderer
            .render_static(&camera)
            .map_err(|e| MapliError::RenderFailed(e.to_string()))?;

        // Copy into an owned, Send buffer that can leave this thread
        Ok(image.as_image().clone())
    }

    fn render_tile(&mut self, req: TileRequest) -> Result<RgbaImage> {
        let key = (req.style.clone(), req.tile_size, req.pixel_ratio);

        let styles = &self.styles;
        let renderer = self.tiles.get_or_try_insert_with(key, || {
            let style = lookup_style(styles, &req.style)?;
            // tile renderers use the width as the tile size, so the size must be square
            let mut renderer = ImageRendererBuilder::new()
                .with_size(req.tile_size, req.tile_size)
                .with_pixel_ratio(req.pixel_ratio.get())
                .build_tile_renderer();
            load_style(&mut renderer, &req.style, &style)?;
            Ok(renderer)
        })?;

        let image = renderer
            .render_tile(req.tile.z(), req.tile.x(), req.tile.y())
            .map_err(|e| MapliError::RenderFailed(e.to_string()))?;

        Ok(image.as_image().clone())
    }
}

fn lookup_style(styles: &StyleRegistry, id: &StyleId) -> Result<Arc<Style>> {
    let map = styles.read().unwrap_or_else(|p| p.into_inner());
    map.get(id)
        .cloned()
        .ok_or_else(|| MapliError::UnknownStyle(id.clone()))
}

fn load_style<M>(renderer: &mut ImageRenderer<M>, id: &StyleId, style: &Style) -> Result<()> {
    let fail = |message: String| MapliError::StyleLoadFailed {
        id: id.clone(),
        message,
    };
    let request = match style {
        Style::Url(url) => renderer.load_style_from_url(url),
        Style::Path(path) => renderer
            .load_style_from_path(path)
            .map_err(|e| fail(e.to_string()))?,
        Style::Json(json) => renderer.load_style_from_json_str(json),
    };
    request.wait().map_err(|e| fail(e.to_string()))
}

struct RendererCache<K, V> {
    entries: HashMap<K, (V, u64)>,
    clock: u64,
    capacity: usize,
}

impl<K: Eq + Hash + Clone, V> RendererCache<K, V> {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            capacity,
        }
    }

    /// Get the value for `key`, creating it with `create` if missing.
    ///
    /// This uses `contains_key` and then `get_mut` instead of the more idiomatic
    /// `entry()` API. This is because evicting needs `&mut self.entries` while an
    /// `Entry` would still be borrowing it.
    fn get_or_try_insert_with(
        &mut self,
        key: K,
        create: impl FnOnce() -> Result<V>,
    ) -> Result<&mut V> {
        self.clock += 1;
        let now = self.clock;
        if !self.entries.contains_key(&key) {
            let value = create()?;
            if self.entries.len() >= self.capacity {
                self.evict_lru();
            }
            self.entries.insert(key.clone(), (value, now));
        }

        let entry = self
            .entries
            .get_mut(&key)
            .expect("entry was just checked or inserted");
        entry.1 = now;
        Ok(&mut entry.0)
    }

    fn evict_lru(&mut self) {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, (_, last_used))| *last_used)
            .map(|(k, _)| k.clone());
        if let Some(k) = oldest {
            // renderer is dropped here, on the woker thread
            self.entries.remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_evicts_least_recently_used() {
        let mut cache: RendererCache<&str, u32> = RendererCache::new(2);
        cache.get_or_try_insert_with("a", || Ok(1)).unwrap();
        cache.get_or_try_insert_with("b", || Ok(2)).unwrap();
        cache.get_or_try_insert_with("a", || Ok(99)).unwrap(); // touch "a"
        cache.get_or_try_insert_with("c", || Ok(3)).unwrap(); // evicts "b"

        assert!(cache.entries.contains_key("a"));
        assert!(!cache.entries.contains_key("b"));
        assert!(cache.entries.contains_key("c"));
        assert_eq!(cache.entries["a"].0, 1); // "a" was not recreated
    }

    #[test]
    fn failed_creation_does_not_evict() {
        let mut cache: RendererCache<&str, u32> = RendererCache::new(1);
        cache.get_or_try_insert_with("a", || Ok(1)).unwrap();
        let res =
            cache.get_or_try_insert_with("b", || Err(MapliError::RenderFailed("boom".into())));
        assert!(res.is_err());
        assert!(cache.entries.contains_key("a"));
    }
}
