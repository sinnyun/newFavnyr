use super::*;

/// Decoding job for a thumbnail, handed to the background worker.
pub(in crate::bridge) struct ThumbJob {
    pub(in crate::bridge) path: PathBuf,
    pub(in crate::bridge) kind: FileKind,
    /// SVGs are loaded by Slint (resvg) on the event loop; other
    /// formats go through `generate_thumb` on the worker.
    pub(in crate::bridge) svg: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::bridge) struct ThumbLocation {
    pub(in crate::bridge) panel: usize,
    pub(in crate::bridge) row: usize,
    pub(in crate::bridge) row_count: usize,
}

pub(in crate::bridge) struct ThumbRequest {
    pub(in crate::bridge) job: ThumbJob,
    pub(in crate::bridge) locations: Vec<ThumbLocation>,
}

pub(in crate::bridge) struct ScheduledThumb {
    pub(in crate::bridge) job: ThumbJob,
    pub(in crate::bridge) locations: Vec<ThumbLocation>,
    pub(in crate::bridge) serial: i32,
}

pub(in crate::bridge) struct InFlightThumb {
    pub(in crate::bridge) locations: Vec<ThumbLocation>,
    pub(in crate::bridge) serial: i32,
}

pub(in crate::bridge) struct ThumbWork {
    pub(in crate::bridge) job: ThumbJob,
    pub(in crate::bridge) serial: i32,
}

impl std::ops::Deref for ThumbWork {
    type Target = ThumbJob;

    fn deref(&self) -> &Self::Target {
        &self.job
    }
}

/// Sort key of the queue. Visible rows come before the window's
/// neighborhood, which itself comes before the rest of the folder. `distance` stabilizes the order
/// around the visible zone; `panel` and `row` make ties deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::bridge) struct ThumbPriority {
    pub(in crate::bridge) tier: u8,
    pub(in crate::bridge) distance: usize,
    pub(in crate::bridge) panel: usize,
    pub(in crate::bridge) row: usize,
}

#[derive(Default)]
pub(in crate::bridge) struct ThumbQueue {
    pub(in crate::bridge) pending: HashMap<PathBuf, ScheduledThumb>,
    pub(in crate::bridge) ready: BinaryHeap<Reverse<(ThumbPriority, i32, PathBuf)>>,
    /// Current locations of an in-progress decode. They can be enriched
    /// if another view displays the same path while the worker is working.
    pub(in crate::bridge) in_flight: HashMap<PathBuf, InFlightThumb>,
    pub(in crate::bridge) next_serial: i32,
}

/// Priority queue shared with the worker. Unlike a FIFO channel,
/// it can promote newly visible rows and remove paths
/// from a folder that was left before their decoding starts.
pub(in crate::bridge) struct ThumbScheduler {
    pub(in crate::bridge) queue: Mutex<ThumbQueue>,
    /// Small state independent of the heavy queue: the scroll callback can
    /// never afford to wait for a heap of thousands of entries to be rebuilt.
    pub(in crate::bridge) viewports: Mutex<HashMap<usize, (usize, usize)>>,
    pub(in crate::bridge) priorities_dirty: AtomicBool,
    pub(in crate::bridge) wake: Condvar,
    started: AtomicBool,
}

impl ThumbScheduler {
    pub(in crate::bridge) fn new() -> Self {
        Self {
            queue: Mutex::new(ThumbQueue::default()),
            viewports: Mutex::new(HashMap::new()),
            priorities_dirty: AtomicBool::new(false),
            wake: Condvar::new(),
            started: AtomicBool::new(false),
        }
    }

    pub(in crate::bridge) fn start_once(&self) -> bool {
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(in crate::bridge) fn priority_for(
        scheduled: &ScheduledThumb,
        viewports: &HashMap<usize, (usize, usize)>,
    ) -> ThumbPriority {
        scheduled
            .locations
            .iter()
            .map(|loc| {
                let fallback = ThumbPriority {
                    tier: 2,
                    distance: loc.row,
                    panel: loc.panel,
                    row: loc.row,
                };
                let Some(&(raw_first, raw_last)) = viewports.get(&loc.panel) else {
                    return fallback;
                };
                if loc.row_count == 0 {
                    return fallback;
                }
                let first = raw_first.min(loc.row_count - 1);
                let last = raw_last.max(first).min(loc.row_count - 1);
                if loc.row >= first && loc.row <= last {
                    return ThumbPriority {
                        tier: 0,
                        distance: loc.row - first,
                        panel: loc.panel,
                        row: loc.row,
                    };
                }
                let distance = if loc.row < first {
                    first - loc.row
                } else {
                    loc.row - last
                };
                // Preloads one viewport height on each side. This
                // absorbs a normal scroll without delaying a far-away destination.
                let visible_len = last - first + 1;
                ThumbPriority {
                    tier: if distance <= visible_len { 1 } else { 2 },
                    distance,
                    panel: loc.panel,
                    row: loc.row,
                }
            })
            .min()
            .unwrap_or(ThumbPriority {
                tier: 2,
                distance: usize::MAX,
                panel: usize::MAX,
                row: usize::MAX,
            })
    }

    pub(in crate::bridge) fn rebuild_ready(
        queue: &mut ThumbQueue,
        viewports: &HashMap<usize, (usize, usize)>,
    ) {
        queue.ready = queue
            .pending
            .iter()
            .map(|(path, scheduled)| {
                Reverse((
                    Self::priority_for(scheduled, viewports),
                    scheduled.serial,
                    path.clone(),
                ))
            })
            .collect();
    }

    pub(in crate::bridge) fn ensure_ready(&self, queue: &mut ThumbQueue) {
        // An update can arrive during the rebuild. The loop then re-reads
        // the most recent viewport before letting the next job go.
        while self.priorities_dirty.swap(false, Ordering::AcqRel) {
            let viewports = self
                .viewports
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            Self::rebuild_ready(queue, &viewports);
        }
    }

    /// Replaces the full request with that of the panels currently in
    /// preview mode. Paths shared across several views are decoded
    /// only once, while keeping each of their positions for sorting.
    pub(in crate::bridge) fn replace_pending(&self, requests: Vec<ThumbRequest>) {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::mem::take(&mut queue.pending);
        let mut next: HashMap<PathBuf, ScheduledThumb> = HashMap::new();

        for request in requests {
            let path = request.job.path.clone();
            if let Some(in_flight) = queue.in_flight.get_mut(&path) {
                for location in request.locations {
                    if !in_flight.locations.contains(&location) {
                        in_flight.locations.push(location);
                    }
                }
                continue;
            }
            if let Some(existing) = next.get_mut(&path) {
                existing.locations.extend(request.locations);
                continue;
            }
            let serial = old.get(&path).map(|s| s.serial).unwrap_or_else(|| {
                let serial = queue.next_serial;
                queue.next_serial = queue.next_serial.wrapping_add(1).max(1);
                serial
            });
            next.insert(
                path,
                ScheduledThumb {
                    job: request.job,
                    locations: request.locations,
                    serial,
                },
            );
        }

        queue.pending = next;
        queue.ready.clear();
        self.priorities_dirty.store(true, Ordering::Release);
        drop(queue);
        self.wake.notify_all();
    }

    /// Adds a few requests that became visible without re-scanning/rebuilding
    /// the whole gallery. Paths already pending/in-flight are simply
    /// enriched with their new location.
    pub(in crate::bridge) fn merge_pending(&self, requests: Vec<ThumbRequest>) {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let mut priorities_changed = false;
        for request in requests {
            let path = request.job.path.clone();
            if let Some(in_flight) = queue.in_flight.get_mut(&path) {
                for location in request.locations {
                    if !in_flight.locations.contains(&location) {
                        in_flight.locations.push(location);
                    }
                }
                continue;
            }
            if let Some(scheduled) = queue.pending.get_mut(&path) {
                for location in request.locations {
                    if !scheduled.locations.contains(&location) {
                        scheduled.locations.push(location);
                        priorities_changed = true;
                    }
                }
                continue;
            }
            let serial = queue.next_serial;
            queue.next_serial = queue.next_serial.wrapping_add(1).max(1);
            queue.pending.insert(
                path,
                ScheduledThumb {
                    job: request.job,
                    locations: request.locations,
                    serial,
                },
            );
            priorities_changed = true;
        }
        if priorities_changed {
            queue.ready.clear();
            self.priorities_dirty.store(true, Ordering::Release);
        }
        drop(queue);
        if priorities_changed {
            self.wake.notify_all();
        }
    }

    /// Updates the only piece of information that varies during a scroll. No
    /// filesystem access or Slint model access happens here.
    pub(in crate::bridge) fn update_viewport(&self, panel: usize, first: i32, last: i32) {
        let mut viewports = self.viewports.lock().unwrap_or_else(|e| e.into_inner());
        let changed = if first < 0 || last < first {
            viewports.remove(&panel).is_some()
        } else {
            let range = (first as usize, last as usize);
            if viewports.get(&panel).copied() == Some(range) {
                false
            } else {
                viewports.insert(panel, range);
                true
            }
        };
        drop(viewports);
        if changed {
            // The O(n) recomputation is deliberately deferred to the worker: the
            // scroll callback stays O(1), regardless of the folder's size.
            self.priorities_dirty.store(true, Ordering::Release);
            self.wake.notify_all();
        }
    }

    pub(in crate::bridge) fn take_next(&self) -> ThumbWork {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            self.ensure_ready(&mut queue);
            while let Some(Reverse((_, _, path))) = queue.ready.pop() {
                if let Some(scheduled) = queue.pending.remove(&path) {
                    let serial = scheduled.serial;
                    queue.in_flight.insert(
                        path,
                        InFlightThumb {
                            locations: scheduled.locations,
                            serial,
                        },
                    );
                    return ThumbWork {
                        job: scheduled.job,
                        serial,
                    };
                }
            }
            queue = self.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
        }
    }

    #[cfg(test)]
    pub(in crate::bridge) fn try_take_next(&self) -> Option<ThumbWork> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_ready(&mut queue);
        while let Some(Reverse((_, _, path))) = queue.ready.pop() {
            if let Some(scheduled) = queue.pending.remove(&path) {
                let serial = scheduled.serial;
                queue.in_flight.insert(
                    path,
                    InFlightThumb {
                        locations: scheduled.locations,
                        serial,
                    },
                );
                return Some(ThumbWork {
                    job: scheduled.job,
                    serial,
                });
            }
        }
        None
    }

    /// Acknowledges a UI success or a decode failure, then frees the worker.
    pub(in crate::bridge) fn complete(&self, path: &Path, serial: i32) {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue
            .in_flight
            .get(path)
            .is_some_and(|in_flight| in_flight.serial == serial)
        {
            queue.in_flight.remove(path);
        }
        drop(queue);
        self.wake.notify_all();
    }

    pub(in crate::bridge) fn wait_until_complete(&self, path: &Path, serial: i32) {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        while queue
            .in_flight
            .get(path)
            .is_some_and(|in_flight| in_flight.serial == serial)
        {
            queue = self.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(in crate::bridge) fn in_flight_locations(
        &self,
        path: &Path,
        serial: i32,
    ) -> Vec<ThumbLocation> {
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .in_flight
            .get(path)
            .filter(|in_flight| in_flight.serial == serial)
            .map(|in_flight| in_flight.locations.clone())
            .unwrap_or_default()
    }

    /// Drops queued/in-flight generations for paths whose contents changed.
    /// A worker may still finish an old decode, but its serial can no longer
    /// match a later request for the same path, so the UI safely discards it.
    pub(in crate::bridge) fn invalidate_paths(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let mut changed = false;
        for path in paths {
            changed |= queue.pending.remove(path).is_some();
            changed |= queue.in_flight.remove(path).is_some();
        }
        if changed {
            queue.ready.clear();
            self.priorities_dirty.store(true, Ordering::Release);
        }
        drop(queue);
        if changed {
            self.wake.notify_all();
        }
    }
}

/// **In-memory** LRU cache (full path → image), bounded by entry count.
/// Session only: nothing is written to disk (privacy constraint).
// Paths whose contents may have changed are evicted explicitly, while
// unrelated folders stay hot.
pub(in crate::bridge) struct ThumbLru {
    map: HashMap<String, Image>,
    order: VecDeque<String>,
    cap: usize,
}

impl ThumbLru {
    pub(in crate::bridge) fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap,
        }
    }
    pub(in crate::bridge) fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.to_string());
    }
    pub(in crate::bridge) fn get(&mut self, key: &str) -> Option<Image> {
        let img = self.map.get(key).cloned()?;
        self.touch(key);
        Some(img)
    }
    /// Test without cloning or promoting the image. Global scans can thus
    /// skip off-screen rows already in cache without skewing LRU recency:
    /// only textures actually reused on screen call `get`.
    pub(in crate::bridge) fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
    pub(in crate::bridge) fn put(&mut self, key: String, img: Image) {
        if !self.map.contains_key(&key)
            && self.map.len() >= self.cap
            && let Some(old) = self.order.pop_front()
        {
            self.map.remove(&old);
        }
        self.map.insert(key.clone(), img);
        self.touch(&key);
    }

    pub(in crate::bridge) fn remove_path(&mut self, path: &Path) {
        let key = path.to_string_lossy();
        self.map.remove(key.as_ref());
        if let Some(position) = self.order.iter().position(|item| item == key.as_ref()) {
            self.order.remove(position);
        }
    }
}
