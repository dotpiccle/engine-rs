//! xtask — automation for the piccle engine workspace.
//!
//! Run with `cargo xtask <command>` (or via cargo aliases like `cargo setup`).
//! Commands include:
//! - `setup`       — project onboarding: install git hooks (via cargo-husky),
//!   check required tools, sync spec submodule
//! - `conformance` — run the spec test-vector conformance suite
//! - `sync-spec`   — re-pin the spec submodule
//! - `bench`       — run the canonical render benchmarks

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo xtask <command>");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  setup        — project onboarding (hooks, tools, submodule)");
        eprintln!("  conformance  — run spec conformance suite (not yet implemented)");
        eprintln!("  sync-spec    — re-pin spec submodule (not yet implemented)");
        eprintln!("  bench        — run benchmarks (not yet implemented)");
        std::process::exit(1);
    }

    match args[1].as_str() {
        "setup" => cmd_setup(),
        "conformance" | "sync-spec" | "bench" => {
            eprintln!("Command '{0}' is not yet implemented.", args[1]);
            std::process::exit(1);
        }
        other => {
            eprintln!("Unknown command: {other}");
            std::process::exit(1);
        }
    }
}

fn cmd_setup() {
    println!("=== piccle engine setup ===");

    let required_tools = [
        "cargo-audit",
        "cargo-deny",
        "cargo-fuzz",
        "cargo-nextest",
        "cargo-llvm-cov",
        "cargo-hack",
        "cargo-release",
        "cargo-flamegraph",
        "samply",
        "typos",
        "committed",
        "dprint",
        "hyperfine",
    ];

    for &tool in &required_tools {
        if which(tool).is_some() {
            println!("  ✅ {tool}");
        }
        else {
            println!("  ⬜ {tool} — installing...");
            let status = std::process::Command::new("cargo").args(["install", tool]).status();
            match status {
                Ok(s) if s.success() => println!("  ✅ {tool} installed"),
                _ => eprintln!("  ❌ {tool} — install failed (try: cargo install {tool})"),
            }
        }
    }

    println!();
    println!("  Installing git hooks via cargo-husky...");
    // Force rebuild: cargo-husky's build.rs only copies hooks when the crate
    // is compiled. If cached, it skips. Clean ensures a fresh build.
    let _ = std::process::Command::new("cargo").args(["clean", "-p", "cargo-husky"]).status();
    let status = std::process::Command::new("cargo")
        .args(["build", "--tests", "-p", "piccle-core"])
        .status();
    match status {
        Ok(s) if s.success() => println!("  ✅ Git hooks installed"),
        _ => eprintln!("  ❌ Could not install git hooks (try: cargo build -p cargo-husky)"),
    }

    println!();
    println!("  Syncing spec submodule...");
    let status = std::process::Command::new("git")
        .args(["submodule", "update", "--init", "--recursive"])
        .status();
    match status {
        Ok(s) if s.success() => println!("  ✅ Spec submodule synced"),
        _ => eprintln!("  ⚠️  Could not sync spec submodule"),
    }

    println!();
    println!("  Checking nightly toolchain...");
    let nightly = std::process::Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.contains("nightly"))
        .unwrap_or(false);
    if nightly {
        println!("  ✅ nightly toolchain present");
        let _ = std::process::Command::new("rustup")
            .args(["component", "add", "--toolchain", "nightly", "rustfmt"])
            .status();
    }
    else {
        println!("  ⬜ nightly toolchain — installing...");
        let s =
            std::process::Command::new("rustup").args(["toolchain", "install", "nightly"]).status();
        if s.is_ok_and(|s| s.success()) {
            println!("  ✅ nightly installed");
            let _ = std::process::Command::new("rustup")
                .args(["component", "add", "--toolchain", "nightly", "rustfmt"])
                .status();
        }
        else {
            eprintln!("  ❌ nightly — install failed (try: rustup toolchain install nightly)");
        }
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
            println!("  ⬜ {target} — installing...");
            let s = std::process::Command::new("rustup").args(["target", "add", target]).status();
            if s.is_ok_and(|s| s.success()) {
                println!("  ✅ {target} installed");
            }
            else {
                eprintln!("  ❌ {target} — install failed (try: rustup target add {target})");
            }
        }
    }

    println!();
    println!("✅ Setup complete.");
    println!("   Next: run `cargo test --workspace` to verify.");
}

fn which(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}
