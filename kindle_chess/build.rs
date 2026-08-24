use dotenv::from_filename;
use std::env;
use std::process::Command;

fn main() {
    get_env_variables();
    emit_build_metadata();
}

// Bakes a short git SHA and an ISO-8601 UTC build timestamp into the binary
// as compile-time `env!()` constants, exposed to the runtime via `crate::version`.
//
// We shell out to `git` and `date` rather than pulling in a build-dep crate —
// both are guaranteed-present on every host we build from (local Linux dev
// machine, `cross` Docker image, GitHub Actions Ubuntu runners).
fn emit_build_metadata() {
    let git_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let build_timestamp = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_SHA={}", git_sha);
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", build_timestamp);

    // Refresh SHA when HEAD moves or the index changes (commit / checkout /
    // stage). `.git` lives at the repo root — one level above the crate.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
}

fn get_env_variables() -> () {
    let profile = std::env::var("PROFILE").unwrap();
    let env_file = match profile.as_str() {
        "release" => ".env.release",
        _ => ".env.debug",
    };

    // Without these, cargo watches only the triggers declared in
    // `emit_build_metadata` and never notices an edited .env file — the stale
    // ROOT_DIR stays baked into the binary until something else forces the
    // build script to re-run. Both files are declared so switching profiles
    // is covered too.
    println!("cargo:rerun-if-changed=.env.debug");
    println!("cargo:rerun-if-changed=.env.release");

    from_filename(env_file).expect(&format!("Failed to load {}", env_file));

    let root_dir = env::var("ROOT_DIR").expect("ROOT_DIR must be set");

    // Pass to compiler
    println!("cargo:rustc-env=ROOT_DIR={}", root_dir);
    println!("cargo:rustc-env=LOG_FILE_DIR={}{}", root_dir, "log/");
    println!("cargo:rustc-env=ASSETS_DIR={}{}", root_dir, "assets/");
    println!(
        "cargo:rustc-env=AUTH_TOKEN={}{}",
        root_dir, "secrets/token.json"
    );
    println!(
        "cargo:rustc-env=LICHESS_API_BASE={}",
        "https://lichess.org/api"
    );
}
