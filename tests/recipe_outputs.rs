//! Managed producer/consumer integration: actual Recipe CLI and real file trees.
//! Build `recipe` explicitly and configure the real GINUX_BIN before running.

use distro_builder::recipe::rootfs_source::{
    materialize_rootfs_from_recipe, RootfsSourceRecipeSpec,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

fn produce_rootfs(case: &str) -> (tempfile::TempDir, anyhow::Result<std::path::PathBuf>) {
    let dir = tempfile::Builder::new()
        .prefix("recipe outputs ")
        .tempdir()
        .unwrap();
    let recipe = dir.path().join("producer.rhai");
    let declaration = match case {
        "valid" => "ctx.outputs.rootfs = root;",
        "missing" => "",
        "wrong-kind" => "ctx.outputs.rootfs = join_path(root, \"usr/bin/contract-tool\");",
        "missing-artifact" => {
            "ctx.outputs.rootfs = join_path(RUNTIME.paths.output, \"not-produced\");"
        }
        _ => panic!("unknown integration case"),
    };
    let source = format!(
        r##"
const RECIPE_API = 2;
let root = join_path(RUNTIME.paths.output, "explicit-rootfs");
let ctx = #{{name: "producer-contract", root: root, outputs: #{{}}}};
{declaration}
fn is_installed(ctx) {{
    if !is_file(join_path(ctx.root, "usr/bin/contract-tool")) {{ return needs_work(ctx, "rootfs missing"); }}
    ready(ctx)
}}
fn is_built(ctx) {{ ready(ctx) }}
fn install(ctx) {{
    let bin = join_path(ctx.root, "usr/bin"); mkdir(bin);
    let tool = join_path(bin, "contract-tool");
    write_file(tool, "#!/bin/sh\nprintf 'real producer output\\n'\n"); chmod(tool, 0o755);
    ctx
}}
"##
    );
    std::fs::write(&recipe, source).unwrap();
    let work = dir.path().join("work");
    // A stale conventional rootfs must not substitute for a missing declaration.
    std::fs::create_dir_all(work.join("rootfs/usr")).unwrap();
    std::fs::write(work.join("rootfs/stale-marker"), "wrong artifact").unwrap();
    // Nor may the subprocess consumer reuse the old fixed JSON-result filename.
    let stale = work.join(".recipe-output.json");
    std::fs::write(&stale, b"{\"rootfs_path\":\"/not-an-output\"}").unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let result = materialize_rootfs_from_recipe(
        repo,
        &work,
        &RootfsSourceRecipeSpec {
            recipe_script: recipe,
            defines: BTreeMap::new(),
        },
    );
    assert_eq!(
        std::fs::read(&stale).unwrap(),
        b"{\"rootfs_path\":\"/not-an-output\"}"
    );
    let result_files = std::fs::read_dir(&work)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".recipe-result.")
        })
        .count();
    assert_eq!(
        result_files, 0,
        "invocation-owned result files must be removed"
    );
    (dir, result)
}

#[test]
fn managed_rootfs_consumer_uses_the_named_real_producer_output() {
    let (_dir, result) = produce_rootfs("valid");
    let rootfs = result.unwrap();
    assert!(!rootfs.join("stale-marker").exists());
    let tool = Command::new(rootfs.join("usr/bin/contract-tool"))
        .output()
        .unwrap();
    assert!(tool.status.success());
    assert_eq!(tool.stdout, b"real producer output\n");
}

#[test]
fn managed_rootfs_consumer_rejects_missing_wrong_kind_and_absent_products() {
    for case in ["missing", "wrong-kind", "missing-artifact"] {
        let (_dir, result) = produce_rootfs(case);
        let error = format!(
            "{:#}",
            result.expect_err("invalid producer output must fail")
        );
        assert!(error.contains("ctx.outputs"), "{case}: {error}");
        if case != "missing" {
            assert!(error.contains("rootfs"), "{case}: {error}");
        }
    }
}

#[test]
fn real_cli_success_without_a_fresh_json_result_cannot_reuse_stale_context() {
    let dir = tempfile::Builder::new()
        .prefix("recipe result ")
        .tempdir()
        .unwrap();
    let recipe = dir.path().join("metadata.rhai");
    std::fs::write(&recipe, "let ctx = #{\n    name: \"metadata-only\",\n};\n").unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let stale = work.join(".recipe-output.json");
    let old_result = b"{\"outputs\":{\"rootfs\":\"/old-result\"}}";
    std::fs::write(&stale, old_result).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let binary = distro_builder::recipe::find_recipe(repo).unwrap();
    // `info` is a real successful CLI operation that produces human metadata,
    // not a lifecycle JSON result. No recipe code is evaluated by this command.
    let error = distro_builder::recipe::run_recipe_phase_json_with_defines(
        &binary.path,
        "info",
        &recipe,
        &work,
        &[],
        Some(dir.path()),
    )
    .expect_err("a successful process without this invocation's JSON must fail");
    assert!(format!("{error:#}").contains("parse this invocation's recipe JSON result"));
    assert_eq!(std::fs::read(stale).unwrap(), old_result);
    assert_eq!(std::fs::read_dir(work).unwrap().count(), 1);
}
