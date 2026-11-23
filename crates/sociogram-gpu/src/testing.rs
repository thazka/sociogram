//! Helpers for tests that need a GPU. Not part of the stable API.
//!
//! The tests assert exact pixels and layout results, which were written
//! against real GPUs. Software rasterizers (lavapipe, llvmpipe, WARP) can differ
//! in details such as anti-aliasing, and CI runners have no GPU, so by default
//! GPU tests run on real adapters only and say so when they skip.
//!
//! Environment variables:
//! - `SOCIOGRAM_REQUIRE_GPU=1`: a missing or software-only adapter is a failure,
//!   not a skip. Use it before pushing so "skipped" can never pass silently.
//! - `SOCIOGRAM_SOFTWARE_GPU=1`: let tests run on software adapters too.

use crate::context::GpuContext;

/// What a GPU test should do on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Run,
    Skip(&'static str),
    Fail(&'static str),
}

/// Decide from the adapter found (`None` = no adapter, `Some(true)` = software
/// adapter, `Some(false)` = real GPU) and the two environment switches.
pub fn decide(
    adapter_is_software: Option<bool>,
    allow_software: bool,
    require_gpu: bool,
) -> Decision {
    match adapter_is_software {
        None if require_gpu => Decision::Fail("no GPU adapter, but SOCIOGRAM_REQUIRE_GPU is set"),
        None => Decision::Skip("no GPU adapter"),
        Some(true) if !allow_software && require_gpu => {
            Decision::Fail("only a software adapter was found, but SOCIOGRAM_REQUIRE_GPU is set")
        }
        Some(true) if !allow_software => {
            Decision::Skip("software adapter (set SOCIOGRAM_SOFTWARE_GPU=1 to run on it)")
        }
        Some(_) => Decision::Run,
    }
}

fn flag(name: &str) -> bool {
    matches!(std::env::var(name).as_deref(), Ok("1" | "true" | "yes"))
}

/// A context for a GPU test, or `None` when the test should be skipped (the
/// reason is printed). Panics when `SOCIOGRAM_REQUIRE_GPU` demands a GPU that
/// is not there.
pub async fn test_context() -> Option<GpuContext> {
    let context = GpuContext::new_headless().await.ok();
    let decision = decide(
        context.as_ref().map(GpuContext::is_software),
        flag("SOCIOGRAM_SOFTWARE_GPU"),
        flag("SOCIOGRAM_REQUIRE_GPU"),
    );
    match decision {
        Decision::Run => context,
        Decision::Skip(why) => {
            eprintln!("skipping GPU test: {why}");
            None
        }
        Decision::Fail(why) => panic!("GPU test environment problem: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_gpu_always_runs() {
        for (allow, require) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_eq!(decide(Some(false), allow, require), Decision::Run);
        }
    }

    #[test]
    fn no_adapter_skips_unless_a_gpu_is_required() {
        assert!(matches!(decide(None, false, false), Decision::Skip(_)));
        assert!(matches!(decide(None, true, false), Decision::Skip(_)));
        assert!(matches!(decide(None, false, true), Decision::Fail(_)));
        assert!(matches!(decide(None, true, true), Decision::Fail(_)));
    }

    #[test]
    fn software_adapters_are_skipped_by_default() {
        assert!(matches!(
            decide(Some(true), false, false),
            Decision::Skip(_)
        ));
    }

    #[test]
    fn software_adapters_run_when_allowed_even_if_a_gpu_is_required() {
        assert_eq!(decide(Some(true), true, false), Decision::Run);
        assert_eq!(decide(Some(true), true, true), Decision::Run);
    }

    #[test]
    fn requiring_a_gpu_turns_a_software_only_machine_into_a_failure() {
        assert!(matches!(decide(Some(true), false, true), Decision::Fail(_)));
    }
}
