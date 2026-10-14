//! One decoder and a single replaceable pending request, independent of the UI/controller.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use crate::image_loader::{self, RawRgbaImage, ThumbnailCache};

#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Request {
    pub files: Vec<PathBuf>,
    pub projects: Vec<PathBuf>,
}

pub(crate) type Images = HashMap<PathBuf, RawRgbaImage>;

#[derive(Default)]
struct Mailbox {
    pending: Option<(u64, Request)>,
    generation: u64,
    stopped: bool,
}

pub(crate) struct Loader {
    mailbox: Arc<(Mutex<Mailbox>, Condvar)>,
}

impl Loader {
    pub fn new(deliver: impl Fn(u64, Images, Images, bool) + Send + 'static) -> Self {
        let mailbox = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker = mailbox.clone();
        std::thread::spawn(move || {
            let cache = ThumbnailCache::new();
            loop {
                let (generation, request) = {
                    let (lock, wake) = &*worker;
                    let mut state = lock.lock().unwrap();
                    while state.pending.is_none() && !state.stopped {
                        state = wake.wait(state).unwrap();
                    }
                    if state.stopped {
                        return;
                    }
                    state.pending.take().unwrap()
                };
                let valid = || {
                    let state = worker.0.lock().unwrap();
                    !state.stopped && state.generation == generation
                };
                let mut files = Images::new();
                let mut projects = Images::new();
                for path in request.projects {
                    if !valid() {
                        break;
                    }
                    if let Some(image) = image_loader::project_logo_raw(&path, &cache) {
                        projects.insert(path, image);
                    }
                }
                if valid() {
                    deliver(generation, files.clone(), projects.clone(), false);
                }
                for (index, path) in request.files.into_iter().enumerate() {
                    if !valid() {
                        break;
                    }
                    if image_loader::is_image_file(&path) {
                        if let Some(image) = cache.get_or_create(&path) {
                            files.insert(path, image);
                        }
                    }
                    if index % 16 == 15 && valid() {
                        deliver(generation, files.clone(), projects.clone(), false);
                    }
                }
                if valid() {
                    deliver(generation, files, projects, true);
                }
            }
        });
        Self { mailbox }
    }

    pub fn request(&self, generation: u64, request: Request) {
        let (lock, wake) = &*self.mailbox;
        let mut state = lock.lock().unwrap();
        state.generation = generation;
        state.pending = Some((generation, request));
        wake.notify_one();
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        let (lock, wake) = &*self.mailbox;
        lock.lock().unwrap().stopped = true;
        wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_reads_files_and_latest_generation_wins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("icon.png");
        image::RgbaImage::from_pixel(80, 40, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let loader = Loader::new(move |generation, files, projects, _| {
            let _ = tx.send((generation, files, projects));
        });
        for generation in 1..=100 {
            loader.request(
                generation,
                Request {
                    files: vec![path.clone()],
                    projects: vec![dir.path().into()],
                },
            );
        }
        loop {
            let (generation, files, projects) =
                rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            if generation != 100 || !files.contains_key(&path) {
                continue;
            }
            assert!(files[&path].width <= 36);
            assert!(projects[dir.path()].width <= 64);
            break;
        }
    }
}
