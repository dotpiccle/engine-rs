//! Fuzz target for the Piccle parser/validator.
//!
//! Per AGENTS.md §10.3 and spec `docs/11-engine-safety.md` §Untrusted input,
//! arbitrary bytes fed to `Validator::check` MUST return `Ok` or `Err` —
//! never panic, never hang, never allocate unbounded memory. The validator's
//! own resource limits (input byte cap, nesting cap) bound allocation.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The only contract: check() returns; it must not panic or abort.
    let _ = piccle_validate::Validator::check(data);
});
