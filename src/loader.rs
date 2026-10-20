//! Background decoding with a small worker pool and an LRU-ish cache, so
//! flipping through a folder of screenshots never blocks the UI.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::SystemTime;

use crate::decode::{self, Decoded};

pub type LoadResult = Result<Arc<Decoded>, String>;

const WORKERS: usize = 3;
const CACHE_ENTRIES: usize = 10;

struct Queue {
    jobs: Mutex<VecDeque<PathBuf>>,
    cv: Condvar,
}

pub struct Loader {
    queue: Arc<Queue>,
    results: Receiver<(PathBuf, Option<SystemTime>, LoadResult)>,
    inflight: HashSet<PathBuf>,
    cache: HashMap<PathBuf, (Option<SystemTime>, LoadResult, u64)>,
    tick: u64,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl Loader {
    pub fn new(ctx: egui::Context) -> Self {
        let queue = Arc::new(Queue { jobs: Mutex::new(VecDeque::new()), cv: Condvar::new() });
        let (tx, results) = mpsc::channel();
        for i in 0..WORKERS {
            let (queue, tx, ctx): (_, Sender<_>, _) = (queue.clone(), tx.clone(), ctx.clone());
            std::thread::Builder::new()
                .name(format!("glint-decode-{i}"))
                .spawn(move || loop {
                    let path = {
                        let mut jobs = queue.jobs.lock().unwrap();
                        loop {
                            if let Some(p) = jobs.pop_front() {
                                break p;
                            }
                            jobs = queue.cv.wait(jobs).unwrap();
                        }
                    };
                    let stamp = mtime(&path);
                    let res = decode::decode(&path).map(Arc::new);
                    if tx.send((path, stamp, res)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                })
                .expect("spawn decoder");
        }
        Self { queue, results, inflight: HashSet::new(), cache: HashMap::new(), tick: 0 }
    }

    /// A cached result that is still current on disk.
    pub fn get(&mut self, path: &Path) -> Option<LoadResult> {
        self.tick += 1;
        let now = mtime(path);
        let tick = self.tick;
        match self.cache.get_mut(path) {
            Some((stamp, res, used)) if *stamp == now => {
                *used = tick;
                Some(res.clone())
            }
            _ => None,
        }
    }

    /// Drop every cached image (used when hiding to the tray).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn invalidate(&mut self, path: &Path) {
        self.cache.remove(path);
    }

    /// Queue a decode. `urgent` jumps the queue (the image the user is waiting for).
    pub fn request(&mut self, path: &Path, urgent: bool) {
        if self.inflight.contains(path) {
            if urgent {
                let mut jobs = self.queue.jobs.lock().unwrap();
                if let Some(i) = jobs.iter().position(|p| p == path) {
                    let p = jobs.remove(i).unwrap();
                    jobs.push_front(p);
                }
            }
            return;
        }
        if self.get(path).is_some() {
            return;
        }
        self.inflight.insert(path.to_path_buf());
        let mut jobs = self.queue.jobs.lock().unwrap();
        if urgent {
            jobs.push_front(path.to_path_buf());
        } else {
            jobs.push_back(path.to_path_buf());
        }
        self.queue.cv.notify_one();
    }

    /// Drop queued prefetches that are no longer near the current image.
    pub fn retain_queued(&mut self, keep: impl Fn(&Path) -> bool) {
        let mut jobs = self.queue.jobs.lock().unwrap();
        jobs.retain(|p| {
            let k = keep(p);
            if !k {
                self.inflight.remove(p);
            }
            k
        });
    }

    /// Collect finished decodes. Returns paths that completed this call.
    pub fn poll(&mut self) -> Vec<PathBuf> {
        let mut done = Vec::new();
        while let Ok((path, stamp, res)) = self.results.try_recv() {
            self.inflight.remove(&path);
            self.tick += 1;
            self.cache.insert(path.clone(), (stamp, res, self.tick));
            done.push(path);
        }
        while self.cache.len() > CACHE_ENTRIES {
            let oldest = self.cache.iter().min_by_key(|(_, (_, _, used))| *used).map(|(p, _)| p.clone());
            if let Some(p) = oldest {
                self.cache.remove(&p);
            }
        }
        done
    }

    pub fn is_loading(&self, path: &Path) -> bool {
        self.inflight.contains(path)
    }
}
