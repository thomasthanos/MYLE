use super::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
struct Count(Arc<AtomicUsize>);
impl engine::Driver for Count {
    fn click(
        &mut self,
        _: Option<model::Point>,
        _: model::MouseButton,
        _: model::ClickType,
    ) -> Result<(), String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}
fn settle(state: &AutoClickerState) -> Status {
    let start = Instant::now();
    while state.status().busy() {
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(2));
    }
    state.status()
}
#[test]
fn stop_during_countdown_does_not_inject_any_clicks() {
    let state = AutoClickerState::default();
    let count = Arc::new(AtomicUsize::new(0));
    state
        .launch(model::Config::default(), Count(count.clone()), 3000)
        .unwrap();
    assert_eq!(state.status().phase, Phase::Countdown);
    assert!(
        state
            .launch(model::Config::default(), Count(count.clone()), 0)
            .is_err()
    );
    state.stop();
    assert_eq!(settle(&state).phase, Phase::Stopped);
    assert_eq!(count.load(Ordering::Relaxed), 0);
}
#[test]
fn worker_completion_and_subsequent_run_have_distinct_ids_and_progress() {
    let state = AutoClickerState::default();
    let count = Arc::new(AtomicUsize::new(0));
    let config = model::Config {
        loops: Some(2),
        interval_ms: 10,
        ..Default::default()
    };
    state
        .launch(config.clone(), Count(count.clone()), 0)
        .unwrap();
    let first = settle(&state);
    assert_eq!(
        (first.phase, first.completed, first.total),
        (Phase::Completed, 2, Some(2))
    );
    state.launch(config, Count(count.clone()), 0).unwrap();
    let second = settle(&state);
    assert!(second.run_id > first.run_id && second.revision > first.revision);
    assert_eq!(second.completed, 2);
    assert_eq!(count.load(Ordering::Relaxed), 4);
}
#[test]
fn picking_and_running_are_mutually_exclusive_and_stale_finish_is_ignored() {
    let state = AutoClickerState::default();
    let (id, _) = state.begin_pick_request(None).unwrap().unwrap();
    assert!(state.begin_pick_request(None).is_err());
    assert!(
        state
            .launch(model::Config::default(), Count(Arc::default()), 0)
            .is_err()
    );
    state.finish(id, Phase::Stopped, None);
    state
        .launch(model::Config::default(), Count(Arc::default()), 3000)
        .unwrap();
    state.finish(id, Phase::Completed, None);
    assert_eq!(state.status().phase, Phase::Countdown);
    state.stop();
    settle(&state);
}
#[test]
fn invalid_configuration_disarms_f6_instead_of_reusing_an_old_profile() {
    let state = AutoClickerState::default();
    state.configure(Some(model::Config::default())).unwrap();
    let invalid = model::Config {
        interval_ms: 0,
        ..Default::default()
    };
    assert!(state.configure(Some(invalid)).is_err());
    assert!(state.0.run.lock().unwrap().prepared.is_none());
}
#[test]
fn stop_preserves_exact_click_count_even_between_throttled_events() {
    struct StopAfterThree {
        state: AutoClickerState,
        count: usize,
    }
    impl engine::Driver for StopAfterThree {
        fn click(
            &mut self,
            _: Option<model::Point>,
            _: model::MouseButton,
            _: model::ClickType,
        ) -> Result<(), String> {
            self.count += 1;
            if self.count == 3 {
                self.state.stop();
            }
            Ok(())
        }
    }
    let state = AutoClickerState::default();
    state
        .launch(
            model::Config {
                loops: None,
                interval_ms: 10,
                ..Default::default()
            },
            StopAfterThree {
                state: state.clone(),
                count: 0,
            },
            0,
        )
        .unwrap();
    assert_eq!(settle(&state).completed, 3);
}
#[test]
fn configuration_delivered_during_a_run_is_used_for_the_next_f6_start() {
    let state = AutoClickerState::default();
    state
        .launch(model::Config::default(), Count(Arc::default()), 3000)
        .unwrap();
    let next = model::Config {
        interval_ms: 250,
        ..Default::default()
    };
    state.configure(Some(next.clone())).unwrap();
    assert_eq!(state.0.run.lock().unwrap().prepared, Some(next));
    state.configure(None).unwrap();
    assert!(state.0.run.lock().unwrap().prepared.is_none());
    state.stop();
    settle(&state);
}
#[test]
fn a_cancelled_pick_request_cannot_install_a_hook_when_its_handler_starts_late() {
    let state = AutoClickerState::default();
    let request = "d98cc421-253b-405e-8dca-a7039d0366bc";
    state.stop_request(Some(request));
    assert!(state.begin_pick_request(Some(request)).unwrap().is_none());
    assert!(!state.status().busy());
}
#[test]
fn picking_clears_execution_progress_from_an_earlier_run() {
    let state = AutoClickerState::default();
    state
        .launch(
            model::Config {
                loops: Some(1),
                ..Default::default()
            },
            Count(Arc::default()),
            0,
        )
        .unwrap();
    assert_eq!(settle(&state).steps, 1);
    let (id, _) = state.begin_pick_request(None).unwrap().unwrap();
    let status = state.status();
    assert_eq!(
        (status.steps, status.step, status.click, status.completed),
        (0, 0, 0, 0)
    );
    state.finish(id, Phase::Ready, None);
}
