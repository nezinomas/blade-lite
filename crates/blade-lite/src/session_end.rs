//! Hands the session end from the thread that hears it to the app's event
//! loop, and waits there until the loop has quit (ticket 159).

use std::fmt::Display;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use crate::report;

/// Work to run on the event loop.
pub type LoopWork = Box<dyn FnOnce() + Send>;

/// Runs `end` on the event loop through `post`, and waits until it has
/// run, at most `wait`: Windows may end the process once the session end
/// returns. The loop runs it once the app's thread is free again, such as
/// after a rate change, so the app is not borrowed then. A post that
/// fails, an `end` that panics, or one not run in time returns, logged.
pub fn end_on_loop<E: Display>(
    post: impl FnOnce(LoopWork) -> Result<(), E>,
    end: fn(),
    wait: Duration,
) {
    let (done, ended) = mpsc::channel();
    let posted = post(Box::new(move || {
        end();
        let _ = done.send(());
    }));
    if let Err(error) = posted {
        report(&format!(
            "the session end could not reach the app, so the quit preset may not be sent: {error}"
        ));
        return;
    }
    match ended.recv_timeout(wait) {
        Ok(()) => {}
        Err(RecvTimeoutError::Disconnected) => {
            report("the quit at the session end did not finish");
        }
        Err(RecvTimeoutError::Timeout) => report(&format!(
            "the app was busy for {} ms at the session end, so the quit preset may not be sent",
            wait.as_millis()
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::{LoopWork, end_on_loop};

    const WAIT: Duration = Duration::from_secs(5);

    static ENDS: AtomicU32 = AtomicU32::new(0);
    static ENDED_LATE: AtomicBool = AtomicBool::new(false);

    fn count_end() {
        ENDS.fetch_add(1, Ordering::SeqCst);
    }

    fn end_late() {
        std::thread::sleep(Duration::from_millis(200));
        ENDED_LATE.store(true, Ordering::SeqCst);
    }

    fn panics() {
        panic!("a bug in the quit");
    }

    fn never() {
        unreachable!("never posted");
    }

    /// A loop on another thread, busy for a while first.
    fn busy_loop() -> impl FnOnce(LoopWork) -> Result<(), &'static str> {
        |work| {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                work();
            });
            Ok(())
        }
    }

    #[test]
    fn waits_until_the_loop_has_run_the_end() {
        end_on_loop(busy_loop(), end_late, WAIT);

        assert!(ENDED_LATE.load(Ordering::SeqCst));
    }

    #[test]
    fn the_end_runs_once() {
        end_on_loop(busy_loop(), count_end, WAIT);

        assert_eq!(ENDS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_loop_that_never_runs_it_is_waited_for_only_so_long() {
        let kept = Arc::new(Mutex::new(None));
        let keep = Arc::clone(&kept);
        let began = Instant::now();

        end_on_loop(
            move |work| -> Result<(), &str> {
                keep.lock().unwrap().replace(work);
                Ok(())
            },
            never,
            Duration::from_millis(100),
        );

        assert!(began.elapsed() < WAIT);
        assert!(kept.lock().unwrap().is_some());
    }

    #[test]
    fn a_post_that_fails_returns_at_once() {
        let began = Instant::now();

        end_on_loop(|_| Err("no loop"), never, WAIT);

        assert!(began.elapsed() < WAIT);
    }

    #[test]
    fn an_end_that_panics_returns_at_once() {
        let began = Instant::now();

        end_on_loop(
            |work: LoopWork| -> Result<(), &str> {
                std::thread::spawn(work);
                Ok(())
            },
            panics,
            WAIT,
        );

        assert!(began.elapsed() < WAIT);
    }
}
