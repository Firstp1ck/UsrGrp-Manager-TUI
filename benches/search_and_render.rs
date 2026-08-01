//! D7 release benchmark for search plus an immutable Ratatui frame.
//!
//! Run with `cargo bench --bench search_and_render -- --nocapture`. The target
//! uses the standard benchmark harness available under the locked manifest;
//! it deliberately keeps timing evidence out of correctness tests.

use std::time::{Duration, Instant};

use ratatui::{Terminal, backend::TestBackend};
use usrgrp_manager::{
    app::{AppState, InputMode},
    sys::{SystemGroup, SystemUser},
    ui,
};

const DATASET_SIZE: u32 = 10_000;
const SAMPLES: usize = 100;
const D7_RENDER_P95_LIMIT: Duration = Duration::from_millis(16);
const D7_SEARCH_P95_LIMIT: Duration = Duration::from_millis(50);

fn fixture_app() -> AppState {
    let mut app = AppState::new();
    app.users_all = (0..DATASET_SIZE)
        .map(|index| SystemUser {
            uid: 1_000 + index,
            name: format!("user{index:05}"),
            primary_gid: 1_000 + index,
            full_name: Some(format!("Fixture User {index}")),
            home_dir: format!("/home/user{index:05}"),
            shell: "/bin/sh".to_owned(),
        })
        .collect();
    app.groups_all = (0..DATASET_SIZE)
        .map(|index| SystemGroup {
            gid: 1_000 + index,
            name: format!("group{index:05}"),
            // Exercise the configured 100,000-edge membership bound rather
            // than measuring only sparse groups.
            members: (0..10)
                .map(|offset| format!("user{:05}", (index + offset) % DATASET_SIZE))
                .collect(),
        })
        .collect();
    app
}

fn p95(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[((samples.len() - 1) * 95) / 100]
}

fn run_d7_benchmark() {
    let mut app = fixture_app();
    let mut search_samples = Vec::with_capacity(SAMPLES);
    let mut render_samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        app.input_mode = InputMode::SearchUsers;
        app.search_query = format!("user{:05}", sample % DATASET_SIZE as usize);
        let search_start = Instant::now();
        app.sort_and_filter();
        search_samples.push(search_start.elapsed());

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test backend");
        let render_start = Instant::now();
        terminal
            .draw(|frame| ui::render(frame, &app))
            .expect("immutable fixture render");
        render_samples.push(render_start.elapsed());
    }
    let search_p95 = p95(&mut search_samples);
    let render_p95 = p95(&mut render_samples);
    println!(
        "D7 10000-user+10000-group search/render: samples={SAMPLES} search_p95_ms={:.3} limit_ms=50 status={} render_p95_ms={:.3} limit_ms=16 status={}",
        search_p95.as_secs_f64() * 1_000.0,
        if search_p95 <= D7_SEARCH_P95_LIMIT {
            "PASS"
        } else {
            "EXCEEDS"
        },
        render_p95.as_secs_f64() * 1_000.0,
        if render_p95 <= D7_RENDER_P95_LIMIT {
            "PASS"
        } else {
            "EXCEEDS"
        },
    );
}

fn main() {
    run_d7_benchmark();
}

#[test]
fn d7_release_benchmark_prints_numeric_search_and_render_evidence() {
    run_d7_benchmark();
}
