//! Real CLI contract routing. These cases intentionally deny the launcher before
//! recipe evaluation; they verify declarations, not managed acquisition.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn source_fixture() -> tempfile::TempDir {
    let fixture = tempfile::Builder::new()
        .prefix("preseed contract ")
        .tempdir()
        .unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for directory in ["distro-variants/acorn", "distro-builder/recipes"] {
        let source = repo.join(directory);
        let destination = fixture.path().join(directory);
        for entry in walkdir::WalkDir::new(&source) {
            let entry = entry.unwrap();
            let target = destination.join(entry.path().strip_prefix(&source).unwrap());
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target).unwrap();
            } else if entry.file_type().is_file() {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let recipes = fixture.path().join("distro-builder/recipes");
    std::fs::copy(
        recipes.join("alpine-live-source-rootfs.rhai"),
        recipes.join("renamed-materializer.rhai"),
    )
    .unwrap();
    std::fs::copy(
        recipes.join("alpine-preseed-source-assets.rhai"),
        recipes.join("renamed-preseed.rhai"),
    )
    .unwrap();
    fixture
}

fn manifest(root: &Path) -> PathBuf {
    root.join("distro-variants/acorn/ring3/sources.toml")
}

fn run_preseed(root: &Path) -> Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let recipe_bin = std::env::var_os("RECIPE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("target/debug/recipe"));
    Command::new(env!("CARGO_BIN_EXE_distro-builder"))
        .current_dir(root)
        .args(["artifact", "preseed-rootfs-source", "acorn"])
        .env("RECIPE_BIN", recipe_bin)
        .env_remove("GINUX_BIN")
        .env_remove("RECIPE_WORKER")
        .env_remove("RECIPE_INVOCATION")
        .env_remove("RECIPE_BACKEND")
        .output()
        .unwrap()
}

#[test]
fn materializer_filename_alone_does_not_enable_preseeding() {
    let fixture = source_fixture();
    let path = manifest(fixture.path());
    let source = std::fs::read_to_string(&path).unwrap();
    let source = source
        .lines()
        .filter(|line| !line.starts_with("preseed_"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, source).unwrap();
    let output = run_preseed(fixture.path());
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not declare sources.rootfs_source.preseed_recipe_script"),
        "{stderr}"
    );
    assert!(!stderr.contains("Running recipe:"), "{stderr}");
}

#[test]
fn explicit_preseed_survives_renamed_recipe_files_and_requires_ginux() {
    let fixture = source_fixture();
    let path = manifest(fixture.path());
    let source = std::fs::read_to_string(&path)
        .unwrap()
        .replace(
            "alpine-live-source-rootfs.rhai",
            "renamed-materializer.rhai",
        )
        .replace("alpine-preseed-source-assets.rhai", "renamed-preseed.rhai");
    std::fs::write(path, source).unwrap();
    let output = run_preseed(fixture.path());
    assert!(
        !output.status.success(),
        "a missing Ginux launcher must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Running recipe:"), "{stderr}");
    assert!(stderr.contains("renamed-preseed.rhai"), "{stderr}");
    assert!(stderr.contains("GINUX_BIN"), "{stderr}");
    assert!(
        !stderr.contains("alpine-preseed-source-assets.rhai"),
        "{stderr}"
    );
}

#[test]
fn incomplete_preseed_contracts_fail_before_recipe_launch() {
    for case in ["missing-outputs", "missing-recipe", "unknown-kind"] {
        let fixture = source_fixture();
        let path = manifest(fixture.path());
        let source = std::fs::read_to_string(&path).unwrap();
        let source = match case {
            "missing-outputs" => source
                .lines()
                .filter(|line| !line.starts_with("preseed_outputs"))
                .collect::<Vec<_>>()
                .join("\n"),
            "missing-recipe" => source
                .lines()
                .filter(|line| !line.starts_with("preseed_recipe_script"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => source.replace("iso = \"file\"", "iso = \"unknown_kind\""),
        };
        std::fs::write(path, source).unwrap();
        let output = run_preseed(fixture.path());
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        if case == "unknown-kind" {
            assert!(stderr.contains("unknown variant"), "{case}: {stderr}");
        } else {
            assert!(stderr.contains("preseed_outputs"), "{case}: {stderr}");
        }
        assert!(!stderr.contains("Running recipe:"), "{case}: {stderr}");
    }
}
