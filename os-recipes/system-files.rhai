// Writing the system tree's files with their target metadata: the four file
// helpers the package-effects step uses. They are the same as the ones of
// the same name in `installed-system.rhai`, kept here so the package-effects
// step does not import the installed-system policy: its Recipe identity then
// covers the packages and the effects only, and an edit to the system
// configuration never reruns it. Change both copies together.

fn put(root, path, content, mode) {
    let dest = join_path(root, path);
    mkdir(dirname(dest));
    write_file(dest, content);
    target_mode(root, path, mode);
}
fn link(root, path, target) {
    let dest = join_path(root, path);
    mkdir(dirname(dest));
    ln_force(target, dest);
}
fn directory(root, path, mode) {
    mkdir(join_path(root, path));
    target_mode(root, path, mode);
}
// One account's shadow line; the aging fields are the same for every account.
fn shadow_line(name, password) { name + ":" + password + ":20000:0:99999:7:::\n" }
