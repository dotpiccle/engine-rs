//! xtask — automation for the piccle engine workspace.
//!
//! Run with `cargo xtask <command>` (or via cargo aliases like `cargo setup`).
//! Commands include:
//! - `setup`       — project onboarding: configure git hooks, check required
//!   tools, sync spec submodule
//! - `conformance` — run the spec test-vector conformance suite
//! - `sync-spec`   — re-pin the spec submodule
//! - `bench`       — run the canonical preparation and render benchmarks
//! - `device-bench` — build and run the ARMv7 benchmark through ADB

// xtask is internal developer automation, not shipped library code: like the
// (deferred) CLI binary, it may fail fast with expect/panic on broken dev
// environments. The no-panic discipline of AGENTS.md §6.4 applies to the
// library crates; here it would only obscure diagnostics.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod conformance;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo xtask <command>");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  setup        — project onboarding (hooks, tools, submodule)");
        eprintln!("  conformance  — run spec conformance suite");
        eprintln!("  sync-spec    — checkout the pinned spec submodule commit");
        eprintln!("  bench        — run the canonical preparation and render benchmarks");
        eprintln!("  device-bench — build and run the ARMv7 benchmark through ADB");
        return ExitCode::FAILURE;
    }

    match args[1].as_str() {
        "setup" => cmd_setup(),
        "conformance" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            let spec = match rest.as_slice() {
                [] => std::path::PathBuf::from("piccle-spec"),
                [flag, path] if flag == "--piccle-spec" => std::path::PathBuf::from(path),
                _ => {
                    eprintln!("Usage: cargo xtask conformance [--piccle-spec <path>]");
                    return ExitCode::FAILURE;
                }
            };
            ExitCode::from(conformance::run(&spec) as u8)
        }
        "sync-spec" => cmd_sync_spec(),
        "bench" => cmd_bench(),
        "device-bench" => cmd_device_bench(),
        other => {
            eprintln!("Unknown command: {other}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_device_bench() -> ExitCode {
    if !command_succeeds("cargo", &["ndk", "--version"]) {
        eprintln!("cargo-ndk is required; install it with `cargo install cargo-ndk --locked`");
        return ExitCode::FAILURE;
    }
    let Some(device) = connected_android_device()
    else {
        eprintln!("Exactly one authorized Android device must be connected through ADB");
        return ExitCode::FAILURE;
    };
    print_android_device_context(&device);

    println!("Building ARMv7 device benchmark for {device}...");
    if !command_succeeds_visible(
        "cargo",
        &[
            "ndk",
            "-t",
            "armeabi-v7a",
            "-p",
            "21",
            "build",
            "--release",
            "-p",
            "xtask",
            "--bin",
            "piccle-device-bench",
        ],
    ) {
        eprintln!("Could not build the ARMv7 device benchmark");
        return ExitCode::FAILURE;
    }

    let local = "target/armv7-linux-androideabi/release/piccle-device-bench";
    let remote = "/data/local/tmp/piccle-device-bench";
    if !command_succeeds_visible("adb", &["-s", &device, "push", local, remote])
        || !command_succeeds("adb", &["-s", &device, "shell", "chmod", "755", remote])
    {
        eprintln!("Could not install the benchmark on {device}");
        return ExitCode::FAILURE;
    }

    println!("Running on-device benchmark on {device}...");
    if !command_succeeds_visible("adb", &["-s", &device, "shell", remote]) {
        eprintln!("The on-device benchmark failed");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn print_android_device_context(device: &str) {
    println!("device_serial\t{device}");
    for (label, property) in [
        ("device_model", "ro.product.model"),
        ("android_version", "ro.build.version.release"),
        ("device_abi", "ro.product.cpu.abi"),
    ] {
        let value = adb_shell_output(device, &["getprop", property])
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unavailable".to_owned());
        println!("{label}\t{value}");
    }
    let memory = adb_shell_output(device, &["cat", "/proc/meminfo"])
        .and_then(|output| {
            output
                .lines()
                .find(|line| line.starts_with("MemTotal:"))
                .map(|line| line.trim().to_owned())
        })
        .unwrap_or_else(|| "unavailable".to_owned());
    println!("device_memory\t{memory}");
}

fn adb_shell_output(device: &str, command: &[&str]) -> Option<String> {
    let output = std::process::Command::new("adb")
        .args(["-s", device, "shell"])
        .args(command)
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn connected_android_device() -> Option<String> {
    let output = std::process::Command::new("adb").args(["devices", "-l"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    parse_connected_android_device(&text)
}

fn parse_connected_android_device(output: &str) -> Option<String> {
    let devices = output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            let serial = fields.next()?;
            (fields.next()? == "device").then(|| serial.to_owned())
        })
        .collect::<Vec<_>>();
    match devices.as_slice() {
        [device] => Some(device.clone()),
        _ => None,
    }
}

/// Runs the canonical preparation and render benchmarks (AGENTS.md §9).
fn cmd_bench() -> ExitCode {
    for args in [
        ["bench", "-p", "piccle-dsp", "--bench", "reverb_prepare"],
        ["bench", "-p", "piccle-render", "--bench", "render"],
    ] {
        let status = std::process::Command::new("cargo")
            .args(args)
            .status()
            .expect("cargo bench must launch");
        if !status.success() {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Checks out the spec submodule at the commit pinned in the index. This is
/// deliberately NOT `--remote`: the pin is the conformance contract and only
/// moves by explicit commit.
fn cmd_sync_spec() -> ExitCode {
    let status = std::process::Command::new("git")
        .args(["submodule", "update", "--init", "--recursive", "piccle-spec"])
        .status()
        .expect("git submodule update must launch");
    if !status.success() {
        eprintln!("  ❌ Could not sync spec submodule");
        return ExitCode::FAILURE;
    }
    let output = std::process::Command::new("git")
        .args(["submodule", "status", "piccle-spec"])
        .output()
        .expect("git submodule status must run");
    println!("  ✅ Spec submodule synced: {}", String::from_utf8_lossy(&output.stdout).trim());
    ExitCode::SUCCESS
}

fn cmd_setup() -> ExitCode {
    println!("=== piccle engine setup ===");
    let mut failures = 0_u32;

    println!("  Checking required developer tools...");
    for tool in [
        "cargo-audit",
        "cargo-deny",
        "cargo-fuzz",
        "cargo-hack",
        "cargo-nextest",
        "cargo-ndk",
        "cargo-llvm-cov",
        "typos",
        "committed",
        "dprint",
    ] {
        if command_available(tool) {
            println!("  ✅ {tool}");
        }
        else {
            failures += 1;
            eprintln!("  ❌ {tool} is missing");
        }
    }

    println!();
    println!("  Configuring repository-owned git hooks...");
    if command_succeeds("git", &["config", "core.hooksPath", ".cargo-husky/hooks"]) {
        println!("  ✅ Git hooks configured");
    }
    else {
        failures += 1;
        eprintln!("  ❌ Could not configure core.hooksPath");
    }

    println!();
    println!("  Syncing pinned spec submodule...");
    if command_succeeds("git", &["submodule", "update", "--init", "--recursive", "piccle-spec"]) {
        println!("  ✅ Spec submodule synced");
    }
    else {
        failures += 1;
        eprintln!("  ❌ Could not sync spec submodule");
    }

    println!();
    println!("  Checking nightly rustfmt...");
    if command_succeeds("rustup", &["run", "nightly", "rustfmt", "--version"]) {
        println!("  ✅ nightly rustfmt present");
    }
    else {
        failures += 1;
        eprintln!("  ❌ Install with: rustup toolchain install nightly --component rustfmt");
    }

    println!();
    println!("  Checking cross-compilation targets...");
    let installed_targets = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let targets = [
        "wasm32-unknown-unknown",
        "aarch64-unknown-linux-gnu",
        "armv7-unknown-linux-gnueabihf",
        "aarch64-linux-android",
        "armv7-linux-androideabi",
        "aarch64-apple-ios",
        "x86_64-apple-ios",
    ];
    for target in &targets {
        if installed_targets.contains(target) {
            println!("  ✅ {target}");
        }
        else {
            failures += 1;
            eprintln!("  ❌ {target} is missing (install with: rustup target add {target})");
        }
    }

    println!();
    if failures > 0 {
        eprintln!("❌ Setup incomplete: {failures} required item(s) need attention.");
        return ExitCode::FAILURE;
    }
    println!("✅ Setup complete. Next: cargo nextest run --workspace --all-features");
    ExitCode::SUCCESS
}

fn command_available(name: &str) -> bool {
    command_succeeds(name, tool_version_args(name))
}

fn tool_version_args(name: &str) -> &'static [&'static str] {
    if name == "cargo-llvm-cov" { &["llvm-cov", "--version"] } else { &["--version"] }
}

fn command_succeeds(name: &str, args: &[&str]) -> bool {
    std::process::Command::new(name)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn command_succeeds_visible(name: &str, args: &[&str]) -> bool {
    std::process::Command::new(name).args(args).status().is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_llvm_cov_uses_its_required_subcommand_for_version_detection() {
        assert_eq!(tool_version_args("cargo-llvm-cov"), ["llvm-cov", "--version"]);
    }

    #[test]
    fn adb_parser_accepts_exactly_one_authorized_device() {
        let output = "List of devices attached\nJ5SERIAL\tdevice product:j5 model:SM_J500M\n";
        assert_eq!(parse_connected_android_device(output).as_deref(), Some("J5SERIAL"));
    }

    #[test]
    fn adb_parser_accepts_space_aligned_platform_tools_output() {
        let output =
            "List of devices attached\nJ5SERIAL            device product:j5 model:SM_J500M\n";
        assert_eq!(parse_connected_android_device(output).as_deref(), Some("J5SERIAL"));
    }

    #[test]
    fn rt60_crossing_finds_the_first_frame_below_the_energy_threshold() {
        assert_eq!(conformance::rt60_crossing(&[1.0, 0.0, 0.0], &[0.0, 0.0, 0.0]), 1);
    }

    #[test]
    fn conformance_rejects_a_path_without_the_spec_layout() {
        assert!(!conformance::is_spec_root(std::path::Path::new("not-a-spec-root")));
    }

    #[test]
    fn stereo_energy_sums_both_channels_across_all_frames() {
        assert_eq!(conformance::stereo_energy(&[1.0, 2.0, 3.0, 4.0]), 30.0);
    }
}
