use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Nv12Planes {
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
    pub y_stride: usize,
    pub uv_stride: usize,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub nv12: Option<Nv12Planes>,
    pub pts: f64,
    pub captured_at: Instant,
    pub serial: u64,
}

pub struct LatestSlot {
    inner: Mutex<Option<Arc<Frame>>>,
    pool: Mutex<Vec<Vec<u8>>>,
    nv12_pool: Mutex<Vec<Nv12Planes>>,
    serial: AtomicU64,
    dropped: AtomicU64,
}

impl Default for LatestSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl LatestSlot {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            pool: Mutex::new(Vec::new()),
            nv12_pool: Mutex::new(Vec::new()),
            serial: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn publish(&self, mut frame: Frame) -> Option<Vec<u8>> {
        frame.serial = self.serial.fetch_add(1, Ordering::SeqCst) + 1;
        let mut slot = self.inner.lock();
        let recycled = match slot.take() {
            Some(old) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                match Arc::try_unwrap(old) {
                    Ok(old) => match old.nv12 {
                        Some(planes) => {
                            self.recycle_planes(planes);
                            None
                        }
                        None => Some(old.rgba),
                    },
                    Err(_) => None,
                }
            }
            None => None,
        };
        *slot = Some(Arc::new(frame));
        recycled
    }

    pub fn take(&self) -> Option<Arc<Frame>> {
        self.inner.lock().take()
    }

    pub fn recycle(&self, frame: Arc<Frame>) {
        let Ok(frame) = Arc::try_unwrap(frame) else {
            return;
        };
        if let Some(planes) = frame.nv12 {
            self.recycle_planes(planes);
            return;
        }
        let mut pool = self.pool.lock();
        if pool.len() < 3 && frame.rgba.capacity() > 0 {
            pool.push(frame.rgba);
        }
    }

    fn recycle_planes(&self, planes: Nv12Planes) {
        let mut pool = self.nv12_pool.lock();
        if pool.len() < 3 {
            pool.push(planes);
        }
    }

    pub fn acquire_buffer(&self) -> Option<Vec<u8>> {
        self.pool.lock().pop()
    }

    pub fn acquire_planes(&self) -> Option<Nv12Planes> {
        self.nv12_pool.lock().pop()
    }

    pub fn clear(&self) {
        *self.inner.lock() = None;
    }

    pub fn reset(&self) {
        *self.inner.lock() = None;
        self.serial.store(0, Ordering::Relaxed);
        self.dropped.store(0, Ordering::Relaxed);
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn serial(&self) -> u64 {
        self.serial.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: usize, height: usize) -> Frame {
        Frame {
            width,
            height,
            rgba: vec![0; width * height * 4],
            nv12: None,
            pts: 0.0,
            captured_at: Instant::now(),
            serial: 0,
        }
    }

    #[test]
    fn newest_frame_replaces_unconsumed_one() {
        let slot = LatestSlot::new();
        slot.publish(frame(2, 2));
        slot.publish(frame(2, 2));
        assert_eq!(slot.dropped(), 1);
        let taken = slot.take().unwrap();
        assert_eq!(taken.serial, 2);
        assert!(slot.take().is_none());
    }

    #[test]
    fn buffers_are_recycled() {
        let slot = LatestSlot::new();
        let first = slot.publish(frame(4, 4));
        assert!(first.is_none());
        let live = slot.take().unwrap();
        slot.recycle(live);
        let buffer = slot.acquire_buffer().unwrap();
        assert_eq!(buffer.len(), 64);
    }

    #[test]
    fn nv12_planes_are_recycled_separately() {
        let slot = LatestSlot::new();
        let mut planes = frame(4, 4);
        planes.nv12 = Some(Nv12Planes {
            y: vec![0; 16],
            uv: vec![0; 8],
            y_stride: 4,
            uv_stride: 4,
        });
        planes.rgba = Vec::new();
        assert!(slot.publish(planes).is_none());
        assert!(slot.acquire_planes().is_none());

        slot.publish(frame(4, 4));
        let recycled = slot.acquire_planes().expect("replaced NV12 frame returns to pool");
        assert_eq!(recycled.y.len(), 16);
        assert_eq!(recycled.uv.len(), 8);
        assert!(slot.acquire_buffer().is_none());
    }
}
