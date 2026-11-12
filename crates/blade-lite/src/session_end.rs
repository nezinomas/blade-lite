//! Hands the session end from the thread that hears it to the app's event
//! loop, and waits there until the loop has quit (ticket 159).

use std::fmt::Display;
use std::panic::catch_unwind;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::report;

/// Work to run on the event loop.
pub type LoopWork = Box<dyn FnOnce() + Send>;

/// Runs `end` on the event loop through `post`, and waits until it has
/// run, at most `wait`: Windows may end the process once the session end
/// returns. The loop runs it once the app's thread is free again, such as
/// after a rate change, so the app is not borrowed then. A post that
/// fails, an `end` that panics, or one not run in time returns, logged.
/// While it waits it calls `serve` every [`SERVE_GAP`], so a Windows call
/// in the quit that sends to every window, such as the rate put back, is
/// not held by this thread (ticket 159 review).
pub fn end_on_loop<E: Display>(
    post: impl FnOnce(LoopWork) -> Result<(), E>,
    end: fn(),
    serve: fn(),
    wait: Duration,
) {
    let (done, ended) = mpsc::channel();
    let posted = post(Box::new(move || {
        // A panic must not unwind into the event loop.
        if catch_unwind(end).is_ok() {
            let _ = done.send(());
        } else {
            report("the session end panicked");
        }
    }));
    if let Err(error) = posted {
        report(&format!(
            "the session end could not reach the app, so the quit preset may not be sent: {error}"
        ));
        return;
    }
    let began = Instant::now();
    loop {
        let left = wait.saturating_sub(began.elapsed());
        match ended.recv_timeout(left.min(SERVE_GAP)) {
            Ok(()) => return,
            Err(RecvTimeoutError::Disconnected) => {
                report("the quit at the session end did not finish");
                return;
            }
            Err(RecvTimeoutError::Timeout) if left > SERVE_GAP => serve(),
            Err(RecvTimeoutError::Timeout) => {
                report(&format!(
                    "the app was busy for {} ms at the session end, so the quit preset may not be sent",
                    wait.as_millis()
                ));
                return;
            }
        }
    }
}

/// How often [`end_on_loop`] serves sent messages while it waits.
pub const SERVE_GAP: Duration = Duration::from_millis(10);

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::{LoopWork, end_on_loop};

    const WAIT: Duration = Duration::from_secs(5);

    static ENDS: AtomicU32 = AtomicU32::new(0);
    static ENDED_LATE: AtomicBool = AtomicBool::new(false);
    static SERVED: AtomicU32 = AtomicU32::new(0);

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

    fn nothing() {}

    fn count_serve() {
        SERVED.fetch_add(1, Ordering::SeqCst);
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
        end_on_loop(busy_loop(), end_late, nothing, WAIT);

        assert!(ENDED_LATE.load(Ordering::SeqCst));
    }

    #[test]
    fn the_end_runs_once() {
        end_on_loop(busy_loop(), count_end, nothing, WAIT);

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
            nothing,
            Duration::from_millis(100),
        );

        assert!(began.elapsed() < WAIT);
        assert!(kept.lock().unwrap().is_some());
    }

    #[test]
    fn a_post_that_fails_returns_at_once() {
        let began = Instant::now();

        end_on_loop(|_| Err("no loop"), never, nothing, WAIT);

        assert!(began.elapsed() < WAIT);
    }

    #[test]
    fn an_end_that_panics_returns_at_once() {
        let began = Instant::now();
        let ran = Arc::new(Mutex::new(None));
        let run = Arc::clone(&ran);

        end_on_loop(
            move |work: LoopWork| -> Result<(), &str> {
                run.lock().unwrap().replace(std::thread::spawn(work));
                Ok(())
            },
            panics,
            nothing,
            WAIT,
        );

        assert!(began.elapsed() < WAIT);
        // Caught in the loop's work, not passed on to the loop.
        let loop_thread = ran.lock().unwrap().take().unwrap();
        assert!(loop_thread.join().is_ok());
    }

    #[test]
    fn sent_messages_are_served_while_it_waits() {
        end_on_loop(busy_loop(), nothing, count_serve, WAIT);

        assert!(SERVED.load(Ordering::SeqCst) > 0);
    }
}
