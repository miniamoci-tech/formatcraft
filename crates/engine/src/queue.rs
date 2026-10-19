//! The task list: a thread-safe queue processed by a small worker pool.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::Error;
use crate::exec::run_job;
use crate::job::Job;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum Status {
    Pending,
    Running,
    Done(PathBuf),
    Failed(String),
    Cancelled,
}

impl Status {
    pub fn is_finished(&self) -> bool {
        matches!(
            self,
            Status::Done(_) | Status::Failed(_) | Status::Cancelled
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryView {
    pub id: u64,
    pub job: Job,
    pub status: Status,
    pub progress: f32,
    #[serde(skip)]
    pub elapsed: Option<Duration>,
}

struct Entry {
    view: EntryView,
    started: Option<Instant>,
    cancel: Arc<AtomicBool>,
}

type Notify = Box<dyn Fn() + Send + Sync>;

struct Shared {
    entries: Mutex<Vec<Entry>>,
    running: AtomicBool,
    workers: AtomicUsize,
    max_workers: AtomicUsize,
    next_id: AtomicU64,
    notify: Mutex<Option<Notify>>,
}

#[derive(Clone)]
pub struct Queue {
    shared: Arc<Shared>,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new(2)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding the lock leaves plain data; keep using it.
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Queue {
    pub fn new(workers: usize) -> Self {
        Queue {
            shared: Arc::new(Shared {
                entries: Mutex::new(Vec::new()),
                running: AtomicBool::new(false),
                workers: AtomicUsize::new(0),
                max_workers: AtomicUsize::new(workers.clamp(1, 16)),
                next_id: AtomicU64::new(1),
                notify: Mutex::new(None),
            }),
        }
    }

    /// Called (from worker threads) whenever something changes; the UI uses
    /// it to request a repaint.
    pub fn set_notify(&self, f: impl Fn() + Send + Sync + 'static) {
        *lock(&self.shared.notify) = Some(Box::new(f));
    }

    pub fn set_workers(&self, n: usize) {
        self.shared
            .max_workers
            .store(n.clamp(1, 16), Ordering::Relaxed);
    }

    pub fn add(&self, job: Job) -> u64 {
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        lock(&self.shared.entries).push(Entry {
            view: EntryView {
                id,
                job,
                status: Status::Pending,
                progress: 0.0,
                elapsed: None,
            },
            started: None,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        self.ping();
        if self.is_running() {
            self.spawn_workers();
        }
        id
    }

    pub fn snapshot(&self) -> Vec<EntryView> {
        lock(&self.shared.entries)
            .iter()
            .map(|e| {
                let mut v = e.view.clone();
                if v.status == Status::Running {
                    v.elapsed = e.started.map(|s| s.elapsed());
                }
                v
            })
            .collect()
    }

    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    pub fn active_workers(&self) -> usize {
        self.shared.workers.load(Ordering::Relaxed)
    }

    pub fn start(&self) {
        self.shared.running.store(true, Ordering::Relaxed);
        self.spawn_workers();
        self.ping();
    }

    /// Stops picking new work and asks running jobs to stop.
    pub fn stop(&self) {
        self.shared.running.store(false, Ordering::Relaxed);
        for e in lock(&self.shared.entries).iter() {
            if e.view.status == Status::Running {
                e.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.ping();
    }

    pub fn cancel(&self, id: u64) {
        let mut entries = lock(&self.shared.entries);
        if let Some(e) = entries.iter_mut().find(|e| e.view.id == id) {
            match e.view.status {
                Status::Running => e.cancel.store(true, Ordering::Relaxed),
                Status::Pending => e.view.status = Status::Cancelled,
                _ => {}
            }
        }
        drop(entries);
        self.ping();
    }

    pub fn remove(&self, id: u64) {
        self.cancel(id);
        lock(&self.shared.entries).retain(|e| e.view.id != id || e.view.status == Status::Running);
        self.ping();
    }

    pub fn retry(&self, id: u64) {
        let mut entries = lock(&self.shared.entries);
        if let Some(e) = entries.iter_mut().find(|e| e.view.id == id)
            && matches!(e.view.status, Status::Failed(_) | Status::Cancelled)
        {
            e.view.status = Status::Pending;
            e.view.progress = 0.0;
            e.cancel = Arc::new(AtomicBool::new(false));
        }
        drop(entries);
        if self.is_running() {
            self.spawn_workers();
        }
        self.ping();
    }

    pub fn clear_finished(&self) {
        lock(&self.shared.entries).retain(|e| !e.view.status.is_finished());
        self.ping();
    }

    fn ping(&self) {
        if let Some(f) = lock(&self.shared.notify).as_ref() {
            f();
        }
    }

    fn spawn_workers(&self) {
        let pending = lock(&self.shared.entries)
            .iter()
            .filter(|e| e.view.status == Status::Pending)
            .count();
        loop {
            let cur = self.shared.workers.load(Ordering::Relaxed);
            let max = self.shared.max_workers.load(Ordering::Relaxed);
            if cur >= max || cur >= pending {
                break;
            }
            if self
                .shared
                .workers
                .compare_exchange(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            let q = self.clone();
            let spawned = std::thread::Builder::new()
                .name("formatcraft-worker".into())
                .spawn(move || q.worker());
            if spawned.is_err() {
                self.shared.workers.fetch_sub(1, Ordering::Relaxed);
                break;
            }
        }
    }

    fn take_next(&self) -> Option<(u64, Job, Arc<AtomicBool>)> {
        if !self.is_running() {
            return None;
        }
        let mut entries = lock(&self.shared.entries);
        let e = entries
            .iter_mut()
            .find(|e| e.view.status == Status::Pending)?;
        e.view.status = Status::Running;
        e.view.progress = 0.0;
        e.started = Some(Instant::now());
        Some((e.view.id, e.view.job.clone(), e.cancel.clone()))
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut Entry)) {
        if let Some(e) = lock(&self.shared.entries)
            .iter_mut()
            .find(|e| e.view.id == id)
        {
            f(e);
        }
        self.ping();
    }

    fn worker(&self) {
        while let Some((id, job, cancel)) = self.take_next() {
            let q = self.clone();
            let progress = move |p: f32| {
                q.update(id, |e| e.view.progress = p.clamp(0.0, 1.0));
            };
            let result = run_job(&job, &progress, &cancel);
            self.update(id, |e| {
                e.view.elapsed = e.started.map(|s| s.elapsed());
                e.view.status = match result {
                    Ok(path) => {
                        e.view.progress = 1.0;
                        Status::Done(path)
                    }
                    Err(Error::Cancelled) => Status::Cancelled,
                    Err(err) => Status::Failed(err.to_string()),
                };
            });
        }
        let left = self.shared.workers.fetch_sub(1, Ordering::Relaxed);
        if left <= 1 {
            let any_pending = lock(&self.shared.entries)
                .iter()
                .any(|e| e.view.status == Status::Pending);
            if !any_pending {
                self.shared.running.store(false, Ordering::Relaxed);
            }
        }
        self.ping();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{ImageOptions, Task};

    #[test]
    fn processes_image_jobs_and_reports_failures() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.png");
        image::RgbImage::new(8, 8).save(&src).unwrap();
        let q = Queue::new(2);
        let ok = Task::ConvertImage {
            input: src,
            format: "jpg".into(),
            options: ImageOptions::default(),
        }
        .into_job(d.path())
        .unwrap();
        let mut bad = ok.clone();
        bad.task = Task::ConvertImage {
            input: d.path().join("missing.png"),
            format: "jpg".into(),
            options: ImageOptions::default(),
        };
        let a = q.add(ok);
        let b = q.add(bad);
        q.start();
        let t0 = Instant::now();
        while q.snapshot().iter().any(|e| !e.status.is_finished()) {
            assert!(t0.elapsed() < Duration::from_secs(20));
            std::thread::sleep(Duration::from_millis(20));
        }
        let s = q.snapshot();
        assert!(matches!(
            s.iter().find(|e| e.id == a).unwrap().status,
            Status::Done(_)
        ));
        assert!(matches!(
            s.iter().find(|e| e.id == b).unwrap().status,
            Status::Failed(_)
        ));
        q.clear_finished();
        assert!(q.snapshot().is_empty());
    }
}
