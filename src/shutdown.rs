use std::sync::atomic::{AtomicI32, Ordering};

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;
static FIRST_SIGNAL: AtomicI32 = AtomicI32::new(0);
static SECOND_SIGNAL: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" {
    fn signal(signal: i32, handler: usize) -> usize;
}

extern "C" fn handle_signal(signal: i32) {
    if FIRST_SIGNAL
        .compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        let _ = SECOND_SIGNAL.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
    }
}

pub(crate) fn install_signal_handlers() -> bool {
    FIRST_SIGNAL.store(0, Ordering::SeqCst);
    SECOND_SIGNAL.store(0, Ordering::SeqCst);
    unsafe {
        signal(SIGINT, handle_signal as *const () as usize) != usize::MAX
            && signal(SIGTERM, handle_signal as *const () as usize) != usize::MAX
    }
}

pub(crate) fn first_signal() -> i32 {
    FIRST_SIGNAL.load(Ordering::SeqCst)
}

pub(crate) fn second_signal() -> i32 {
    SECOND_SIGNAL.load(Ordering::SeqCst)
}

pub(crate) fn second_signal_exit(signal: i32) -> Option<i32> {
    match signal {
        SIGINT => Some(130),
        SIGTERM => Some(143),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShutdownOutcome {
    Clean,
    TimedOut,
    ChildFailed,
}

pub(crate) fn shutdown_exit(outcome: ShutdownOutcome) -> i32 {
    match outcome {
        ShutdownOutcome::Clean => 0,
        ShutdownOutcome::TimedOut => 124,
        ShutdownOutcome::ChildFailed => 1,
    }
}

pub(crate) fn shutdown_timeout_error() -> (&'static str, &'static str) {
    ("lifecycle.shutdown_timeout", "shutdown timed out")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_signal_exit_codes_are_exact() {
        assert_eq!(second_signal_exit(2), Some(130));
        assert_eq!(second_signal_exit(15), Some(143));
        assert_eq!(second_signal_exit(0), None);
    }

    #[test]
    fn shutdown_outcomes_have_locked_exit_codes() {
        assert_eq!(shutdown_exit(ShutdownOutcome::Clean), 0);
        assert_eq!(shutdown_exit(ShutdownOutcome::TimedOut), 124);
        assert_eq!(shutdown_exit(ShutdownOutcome::ChildFailed), 1);
    }

    #[test]
    fn timeout_error_is_stable_and_redacted() {
        assert_eq!(
            shutdown_timeout_error(),
            ("lifecycle.shutdown_timeout", "shutdown timed out")
        );
    }
}
