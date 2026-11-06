// Pinned HTTPS acquisition of the locked Fedora RPM set. The rootfs
// materializer imports the directory as its `rpms` input.
const RECIPE_API = 2;
import "sources/levitate-rpm-lock.rhai" as lock;
let ctx = #{
    name: "fedora-rpm-preseed",
    description: "Locked Fedora " + lock::release() + " RPM set",
    rpms_dir: join_path(RUNTIME.paths.output, "rpms"),
    packages: lock::packages(),
};

fn _rpm_file(ctx, rpm) { join_path(ctx.rpms_dir, basename(rpm[2])) }

fn is_acquired(ctx) {
    for rpm in ctx.packages {
        let file = _rpm_file(ctx, rpm);
        if !is_file(file) { return needs_work(ctx, "missing " + basename(file)); }
        verify_sha256(file, rpm[3]);
    }
    ctx.outputs = #{rpms: ctx.rpms_dir};
    ready(ctx)
}

// Fedora's mirrors drop superseded update builds, so a failed download
// usually means the lock needs refreshing.
fn _download(rpm, file) {
    try {
        download(rpm[2], file, rpm[3]);
    } catch (err) {
        throw "downloading " + rpm[1] + " failed (" + err +
            "); if Fedora replaced that build, rerun `cargo xtask base lock levitate`";
    }
}

fn acquire(ctx) {
    mkdir(ctx.rpms_dir);
    for rpm in ctx.packages {
        let file = _rpm_file(ctx, rpm);
        if ctx.runtime.options.force_acquire || !is_file(file) {
            _download(rpm, file);
        } else {
            verify_sha256(file, rpm[3]);
        }
    }
    ctx.outputs = #{rpms: ctx.rpms_dir};
    ctx
}

fn is_built(ctx) { is_acquired(ctx) }
fn build(ctx) { ctx }
fn is_installed(ctx) { is_acquired(ctx) }
fn install(ctx) { ctx.outputs = #{rpms: ctx.rpms_dir}; ctx }

fn remove(ctx) {
    if is_dir(ctx.rpms_dir) { rm(ctx.rpms_dir); }
    ctx.outputs = #{};
    ctx
}
