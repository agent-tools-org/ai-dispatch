// Shared admission gate: exponential cooldown and paced requests after throttling.
// All workers use one mutex-protected schedule; depends only on std time/threads.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) struct Gate {
    schedule: Mutex<Schedule>,
    base: Duration,
}

struct Schedule {
    next: Instant,
    delay: Duration,
}

impl Gate {
    pub(super) fn new(base: Duration) -> Self {
        Self {
            schedule: Mutex::new(Schedule {
                next: Instant::now(),
                delay: Duration::ZERO,
            }),
            base,
        }
    }

    pub(super) fn enter(&self) {
        loop {
            let wait = {
                let mut schedule = self.schedule.lock().unwrap_or_else(|error| error.into_inner());
                let now = Instant::now();
                if now >= schedule.next {
                    schedule.next = now + schedule.delay;
                    return;
                }
                schedule.next - now
            };
            std::thread::sleep(wait);
        }
    }

    pub(super) fn throttled(&self) {
        let mut schedule = self.schedule.lock().unwrap_or_else(|error| error.into_inner());
        schedule.delay = (schedule.delay * 2).max(self.base).min(self.base * 32);
        schedule.next = schedule.next.max(Instant::now() + schedule.delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_limits_increase_shared_delay_and_cap_it() {
        let gate = Gate::new(Duration::from_millis(1));
        for expected in [1, 2, 4, 8, 16, 32, 32] {
            gate.throttled();
            let schedule = gate.schedule.lock().expect("schedule");
            assert_eq!(schedule.delay, Duration::from_millis(expected));
            assert!(schedule.next > Instant::now());
        }
    }
}
