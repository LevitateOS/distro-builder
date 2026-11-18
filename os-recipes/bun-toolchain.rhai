const RECIPE_API = 1;
// Pinned compiler and target runtime for the TUI producer. Acquisition and
// extraction belong here; the builder only selects the runtime via a typed input.
// The archives to download depend on the runtime_target input, so they are
// downloaded in build, not declared as SOURCES.
const NEEDS = #{
    inputs: #{ runtime_target: "file" },
    outputs: #{ compiler: "file", runtime: "file" },
    network: ["github.com:443", "release-assets.githubusercontent.com:443"],
    network_max_bytes: 536870912,
    memory_mib: 4096,
    timeout_secs: 600,
    runtime: "minimal",
};
const PACKAGE = #{ name: "bun-toolchain", version: "1.4.2", description: "Pinned Bun compiler and target runtime for the TUI producer" };

fn digest(target) {
    if target == "bun-linux-x64" { return "36368faef7527875d5ffa52e53cd48021741f2a83eb6208a8dd64068d422a913"; }
    if target == "bun-linux-x64-musl" { return "4835eca59d6da70f4674f5642f6e459dcadab773695b2ed9922d131057989742"; }
    throw "unsupported Bun target: " + target;
}

fn build(src, out) {
    let target = trim(read_file(INPUTS.runtime_target));
    if target != "bun-linux-x64" && target != "bun-linux-x64-musl" {
        throw "unsupported Bun target: " + target;
    }
    // The compiler always runs on the build host (bun-linux-x64); the runtime is the target's.
    let targets = ["bun-linux-x64"];
    if target != "bun-linux-x64" { targets.push(target); }
    let unpacked = join_path(WORK, "unpacked");
    mkdir(unpacked);
    for name in targets {
        let archive = join_path(WORK, name + ".zip");
        download("https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/" + name + ".zip",
            archive, digest(name));
        extract_with_format(archive, unpacked, "zip");
        if !is_file(join_path(unpacked, name + "/bun")) { throw "Bun archive lacks " + name + "/bun"; }
    }
    copy_file(join_path(unpacked, "bun-linux-x64/bun"), out.compiler);
    copy_file(join_path(unpacked, target + "/bun"), out.runtime);
    chmod(out.compiler, 0o755);
    chmod(out.runtime, 0o755);
}

fn verify(out) {
    if !is_file(out.compiler) { throw "the Bun compiler is missing"; }
    if !is_file(out.runtime) { throw "the Bun runtime is missing"; }
}
