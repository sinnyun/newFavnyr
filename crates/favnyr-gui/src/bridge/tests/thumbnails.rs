use super::*;

fn thumb_test_request(path: &str, panel: usize, row: usize, row_count: usize) -> ThumbRequest {
    ThumbRequest {
        job: ThumbJob {
            path: PathBuf::from(path),
            kind: FileKind::Image,
            svg: false,
        },
        locations: vec![ThumbLocation {
            panel,
            row,
            row_count,
        }],
    }
}

#[test]
fn thumbnail_scroll_reprioritizes_the_next_job() {
    let scheduler = ThumbScheduler::new();
    scheduler.replace_pending(
        (0..20)
            .map(|row| thumb_test_request(&format!("image-{row:02}.png"), 0, row, 20))
            .collect(),
    );
    scheduler.update_viewport(0, 0, 1);

    // The first decode has already started when the user jumps to the bottom.
    let first = scheduler.try_take_next().unwrap();
    assert_eq!(first.path, PathBuf::from("image-00.png"));
    // Several intermediate positions may be published during a
    // fast scroll: only the most recent one should drive the next choice.
    scheduler.update_viewport(0, 7, 8);
    scheduler.update_viewport(0, 12, 13);
    scheduler.update_viewport(0, 18, 19);
    scheduler.complete(&first.path, first.serial);

    // The next one comes from the new visible zone, not the old FIFO.
    let next = scheduler.try_take_next().unwrap();
    assert_eq!(next.path, PathBuf::from("image-18.png"));
    scheduler.complete(&next.path, next.serial);
    let next = scheduler.try_take_next().unwrap();
    assert_eq!(next.path, PathBuf::from("image-19.png"));
    scheduler.complete(&next.path, next.serial);
}

#[test]
fn thumbnail_pool_takes_each_job_exactly_once_under_concurrency() {
    use std::sync::Mutex;
    // Invariant that makes the pool safe: several workers pull in
    // parallel, but `take_next` removes the path from `pending` UNDER LOCK →
    // no path is decoded twice, none is lost. We stress it
    // with lots of jobs and more threads than the real pool.
    const JOBS: usize = 500;
    let scheduler = Arc::new(ThumbScheduler::new());
    scheduler.replace_pending(
        (0..JOBS)
            .map(|row| thumb_test_request(&format!("img-{row:04}.png"), 0, row, JOBS))
            .collect(),
    );

    let taken: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::with_capacity(JOBS)));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let sched = scheduler.clone();
        let taken = taken.clone();
        handles.push(std::thread::spawn(move || {
            // `try_take_next` returns `None` on an empty queue (clean test exit);
            // in production it's `take_next`, blocking, that waits for the next job.
            while let Some(job) = sched.try_take_next() {
                taken
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(job.path.clone());
                sched.complete(&job.path, job.serial);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let mut paths = taken.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(paths.len(), JOBS, "each job taken exactly once");
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), JOBS, "no duplicate decode");
}

#[test]
fn thumbnail_requests_are_deduplicated_and_stale_paths_are_removed() {
    let scheduler = ThumbScheduler::new();
    scheduler.replace_pending(vec![
        thumb_test_request("shared.png", 0, 3, 10),
        thumb_test_request("shared.png", 1, 1, 10),
        thumb_test_request("obsolete.png", 0, 4, 10),
    ]);
    scheduler.replace_pending(vec![
        thumb_test_request("shared.png", 0, 3, 10),
        thumb_test_request("shared.png", 1, 1, 10),
    ]);
    scheduler.update_viewport(1, 1, 2);

    let only = scheduler.try_take_next().unwrap();
    assert_eq!(only.path, PathBuf::from("shared.png"));
    // A refresh during decoding must not create a duplicate.
    scheduler.replace_pending(vec![thumb_test_request("shared.png", 1, 1, 10)]);
    scheduler.complete(&only.path, only.serial);
    assert!(scheduler.try_take_next().is_none());
}

#[test]
fn invalidated_thumbnail_generation_cannot_complete_a_new_request() {
    let scheduler = ThumbScheduler::new();
    let path = PathBuf::from("gallery").join("image-02.jpg");
    scheduler.replace_pending(vec![thumb_test_request(
        path.to_string_lossy().as_ref(),
        0,
        0,
        1,
    )]);
    let stale = scheduler.try_take_next().expect("stale generation");

    scheduler.invalidate_paths(std::slice::from_ref(&path));
    scheduler.merge_pending(vec![thumb_test_request(
        path.to_string_lossy().as_ref(),
        0,
        0,
        1,
    )]);
    let current = scheduler.try_take_next().expect("current generation");
    assert_ne!(stale.serial, current.serial);
    assert!(
        scheduler
            .in_flight_locations(&path, stale.serial)
            .is_empty(),
        "an invalidated decode must lose ownership of the path"
    );
    assert_eq!(
        scheduler.in_flight_locations(&path, current.serial).len(),
        1
    );

    // A late callback from the old decode cannot acknowledge/remove the
    // replacement job that now owns the same path.
    scheduler.complete(&path, stale.serial);
    assert_eq!(
        scheduler.in_flight_locations(&path, current.serial).len(),
        1
    );
    scheduler.complete(&path, current.serial);
}

#[test]
fn thumbnail_lru_removes_only_the_reused_path() {
    let image = image_from_thumb(&Thumbnail {
        width: 1,
        height: 1,
        rgba: vec![1, 2, 3, 255],
    });
    let reused = PathBuf::from("gallery").join("image-02.jpg");
    let untouched = PathBuf::from("gallery").join("image-03.jpg");
    let mut cache = ThumbLru::new(8);
    cache.put(reused.to_string_lossy().into_owned(), image.clone());
    cache.put(untouched.to_string_lossy().into_owned(), image);

    cache.remove_path(&reused);

    assert!(cache.get(reused.to_string_lossy().as_ref()).is_none());
    assert!(cache.get(untouched.to_string_lossy().as_ref()).is_some());
}

#[test]
fn thumbnail_idempotent_merge_keeps_the_ready_heap_intact() {
    let scheduler = ThumbScheduler::new();
    scheduler.replace_pending(vec![thumb_test_request("stable.png", 0, 4, 10)]);
    {
        let mut queue = scheduler.queue.lock().unwrap_or_else(|e| e.into_inner());
        scheduler.ensure_ready(&mut queue);
        assert_eq!(queue.ready.len(), 1);
        assert!(!scheduler.priorities_dirty.load(Ordering::Acquire));
    }

    // Hot path of the viewport callback: the visible row is already known at the
    // same location. No O(n) rebuild, no worker wake-up needed.
    scheduler.merge_pending(vec![thumb_test_request("stable.png", 0, 4, 10)]);
    let queue = scheduler.queue.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(queue.ready.len(), 1);
    assert!(!scheduler.priorities_dirty.load(Ordering::Acquire));
}

#[test]
fn thumbnail_navigation_keeps_new_queue_after_stale_job_finishes() {
    let scheduler = ThumbScheduler::new();
    scheduler.replace_pending(vec![thumb_test_request("old-folder.png", 0, 0, 1)]);
    let stale = scheduler.try_take_next().unwrap();

    // Navigation during decoding: the new request replaces the whole
    // old queue, but the already-started job can finish without polluting it.
    scheduler.replace_pending(
        (0..6)
            .map(|row| thumb_test_request(&format!("new-{row}.png"), 0, row, 6))
            .collect(),
    );
    scheduler.update_viewport(0, 5, 5);
    scheduler.complete(&stale.path, stale.serial);

    let next = scheduler.try_take_next().unwrap();
    assert_eq!(next.path, PathBuf::from("new-5.png"));
    scheduler.complete(&next.path, next.serial);
    assert_ne!(next.path, stale.path);
}

#[test]
fn visible_thumbnail_holes_are_reconciled_when_render_window_is_unchanged() {
    let gallery = PathBuf::from("gallery");
    let state = AppState::new_at(Config::default(), gallery.clone(), 0);
    let (top, height, expected) = {
        let mut panels = state.panels.borrow_mut();
        let panel = &mut panels[0];
        panel.tabs.tabs[0].mode = ViewMode::Previews;
        panel.tabs.tabs[0].zoom = THUMB_DEFAULT_ZOOM;

        let mut rows: Vec<FileRow> = (0..100)
            .map(|index| FileRow {
                name: format!("image-{index:03}.png").into(),
                // A row carries its parent: the thumbnail key is
                // `row.path`, so a subfolder section can hold the entries
                // of another folder.
                path: gallery.display().to_string().into(),
                ext: "png".into(),
                kind: FileKind::Image.as_i32(),
                preview_capable: true,
                ..Default::default()
            })
            .collect();
        layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
        let top = rows[50].visual_y;
        let height = rows[50].visual_h * 2.0;
        let visible = row_range_for_slice(&rows, top, top + height);
        let expected = (visible.0..visible.1)
            .map(|index| gallery.join(format!("image-{index:03}.png")))
            .collect::<Vec<_>>();
        panel.viewport_top.set(top);
        panel.viewport_height.set(height);
        panel.replace_rows(rows);
        // Reproduces the zoom relayout: `replace_rows` has already pre-marked
        // exactly the window that the callback is going to republish.
        assert_eq!(
            (panel.rendered_first.get(), panel.rendered_end.get()),
            row_range_for_content_span(
                &*panel.rows_model,
                (top - height).max(0.0),
                top + height * 2.0,
            )
        );
        (top, height, expected)
    };

    // Same range twice: the second reconciliation must neither lose nor
    // duplicate the already-pending requests.
    update_panel_render_window(&state, 0, top, height);
    update_panel_render_window(&state, 0, top, height);

    let mut actual = Vec::new();
    while let Some(job) = state.thumb_scheduler.try_take_next() {
        actual.push(job.path.clone());
        state.thumb_scheduler.complete(&job.path, job.serial);
    }
    actual.sort();
    let mut expected = expected;
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn cached_offscreen_thumbnails_are_not_redecoded_by_a_global_refresh() {
    let gallery = PathBuf::from("gallery-cache");
    let state = AppState::new_at(Config::default(), gallery.clone(), 0);
    let cached_image = image_from_thumb(&Thumbnail {
        width: 1,
        height: 1,
        rgba: vec![1, 2, 3, 255],
    });
    {
        let mut panels = state.panels.borrow_mut();
        let panel = &mut panels[0];
        panel.tabs.tabs[0].mode = ViewMode::Previews;
        panel.tabs.tabs[0].zoom = THUMB_DEFAULT_ZOOM;
        panel.viewport_height.set(76.0);
        let mut rows: Vec<FileRow> = (0..40)
            .map(|index| FileRow {
                name: format!("cached-{index:02}.png").into(),
                path: gallery.display().to_string().into(),
                ext: "png".into(),
                kind: FileKind::Image.as_i32(),
                preview_capable: true,
                ..Default::default()
            })
            .collect();
        layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
        panel.replace_rows(rows);
    }
    for index in 0..40 {
        let key = gallery
            .join(format!("cached-{index:02}.png"))
            .to_string_lossy()
            .into_owned();
        state
            .thumb_cache
            .borrow_mut()
            .put(key, cached_image.clone());
    }

    request_thumbnails(&state);
    assert!(
        state.thumb_scheduler.try_take_next().is_none(),
        "a texture already in the LRU must not be re-decoded off-screen"
    );
}
