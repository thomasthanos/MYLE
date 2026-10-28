use super::model::{ClickType, Config, Mode, MouseButton, Point};
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
#[derive(Default)]
pub struct Cancel {
    stopped: AtomicBool,
    gate: Mutex<()>,
    wake: Condvar,
}
impl Cancel {
    pub fn stop(&self) {
        let _guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
    }
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
    /// True only when the whole delay elapsed without cancellation.
    pub fn wait(&self, ms: u64) -> bool {
        let guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        let _result = self
            .wake
            .wait_timeout_while(guard, Duration::from_millis(ms), |_| !self.stopped());
        !self.stopped()
    }
}
pub trait Driver {
    fn click(
        &mut self,
        point: Option<Point>,
        button: MouseButton,
        kind: ClickType,
    ) -> Result<(), String>;
}
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub loop_number: u64,
    pub step: usize,
    pub click: u32,
    pub completed: u64,
}
pub fn execute(
    config: &Config,
    cancel: &Cancel,
    driver: &mut impl Driver,
    mut report: impl FnMut(Progress),
) -> Result<(), String> {
    config.validate()?;
    let mut completed = 0u64;
    let mut cycle = 1u64;
    while !cancel.stopped() && config.loops.is_none_or(|n| cycle <= u64::from(n)) {
        match config.mode {
            Mode::Simple => {
                driver.click(None, config.button, config.click_type)?;
                completed = completed.saturating_add(1);
                report(Progress {
                    loop_number: cycle,
                    step: 1,
                    click: 1,
                    completed,
                });
                if config.loops.is_none_or(|n| cycle < u64::from(n))
                    && !cancel.wait(config.interval_ms)
                {
                    break;
                }
            }
            Mode::Sequence => {
                for (index, step) in config.steps.iter().enumerate() {
                    for click in 1..=step.clicks {
                        if cancel.stopped() {
                            return Ok(());
                        }
                        driver.click(
                            Some(Point {
                                x: step.x,
                                y: step.y,
                            }),
                            step.button,
                            step.click_type,
                        )?;
                        completed = completed.saturating_add(1);
                        report(Progress {
                            loop_number: cycle,
                            step: index + 1,
                            click,
                            completed,
                        });
                        if click < step.clicks && !cancel.wait(step.interval_ms) {
                            return Ok(());
                        }
                    }
                    let has_next = index + 1 < config.steps.len()
                        || config.loops.is_none_or(|n| cycle < u64::from(n));
                    if has_next && !cancel.wait(step.delay_ms) {
                        return Ok(());
                    }
                }
            }
        }
        cycle = cycle.saturating_add(1);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoclicker::model::*;
    use std::sync::Arc;
    use std::time::Instant;
    #[derive(Default)]
    struct Recorder {
        clicks: Vec<(Option<Point>, MouseButton, ClickType)>,
    }
    impl Driver for Recorder {
        fn click(
            &mut self,
            point: Option<Point>,
            button: MouseButton,
            kind: ClickType,
        ) -> Result<(), String> {
            self.clicks.push((point, button, kind));
            Ok(())
        }
    }
    #[test]
    fn finite_sequence_counts_actions_and_preserves_order_across_loops() {
        let config = Config {
            mode: Mode::Sequence,
            loops: Some(2),
            steps: vec![
                Step {
                    clicks: 2,
                    x: -100,
                    y: 40,
                    interval_ms: 10,
                    delay_ms: 0,
                    ..Step::default()
                },
                Step {
                    clicks: 1,
                    x: 500,
                    y: 200,
                    button: MouseButton::Right,
                    click_type: ClickType::Double,
                    interval_ms: 10,
                    delay_ms: 0,
                    ..Step::default()
                },
            ],
            ..Config::default()
        };
        let mut driver = Recorder::default();
        let mut progress = Vec::new();
        execute(&config, &Cancel::default(), &mut driver, |p| {
            progress.push(p)
        })
        .unwrap();
        assert_eq!(driver.clicks.len(), 6);
        assert_eq!(
            driver.clicks[0],
            (
                Some(Point { x: -100, y: 40 }),
                MouseButton::Left,
                ClickType::Single
            )
        );
        assert_eq!(
            driver.clicks[2],
            (
                Some(Point { x: 500, y: 200 }),
                MouseButton::Right,
                ClickType::Double
            )
        );
        assert_eq!(driver.clicks[3], driver.clicks[0]);
        let last = progress.last().unwrap();
        assert_eq!(
            (last.loop_number, last.step, last.click, last.completed),
            (2, 2, 1, 6)
        );
    }
    #[test]
    fn simple_mode_uses_live_cursor_and_stops_without_extra_clicks() {
        let cancel = Cancel::default();
        let mut driver = Recorder::default();
        let config = Config {
            loops: None,
            interval_ms: 10,
            ..Config::default()
        };
        execute(&config, &cancel, &mut driver, |p| {
            if p.completed == 3 {
                cancel.stop();
            }
        })
        .unwrap();
        assert_eq!(driver.clicks.len(), 3);
        assert!(driver.clicks.iter().all(|(p, _, _)| p.is_none()));
    }
    #[test]
    fn long_wait_is_interrupted_without_waiting_for_its_timeout() {
        let cancel = Arc::new(Cancel::default());
        let worker_cancel = cancel.clone();
        let start = Instant::now();
        let worker = std::thread::spawn(move || worker_cancel.wait(60_000));
        std::thread::sleep(Duration::from_millis(20));
        cancel.stop();
        assert!(!worker.join().unwrap());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(
            !cancel.wait(60_000),
            "a stop before the wait must not be lost"
        );
    }
    #[test]
    fn cancellation_during_a_transition_never_runs_the_next_step() {
        let cancel = Cancel::default();
        let config = Config {
            mode: Mode::Sequence,
            steps: vec![
                Step {
                    clicks: 1,
                    delay_ms: 60_000,
                    ..Step::default()
                },
                Step::default(),
            ],
            ..Config::default()
        };
        let mut driver = Recorder::default();
        execute(&config, &cancel, &mut driver, |_| cancel.stop()).unwrap();
        assert_eq!(driver.clicks.len(), 1);
    }
    #[test]
    fn driver_failure_stops_the_sequence_without_reporting_a_completed_action() {
        struct Fails;
        impl Driver for Fails {
            fn click(
                &mut self,
                _: Option<Point>,
                _: MouseButton,
                _: ClickType,
            ) -> Result<(), String> {
                Err("blocked".into())
            }
        }
        let mut completed = 0;
        let result = execute(&Config::default(), &Cancel::default(), &mut Fails, |p| {
            completed = p.completed
        });
        assert_eq!(result, Err("blocked".into()));
        assert_eq!(completed, 0);
    }
}
