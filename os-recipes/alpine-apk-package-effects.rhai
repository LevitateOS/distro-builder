//! extends: package-effects.rhai
const RECIPE_API = 1;
// The APK backend of the package-effects step. The composed input is the
// base tree (`alpine-apk-base-tree.rhai`: the base lock's APKs installed by
// the locked apk.static, without their scripts, and captured). The packages'
// scripts never ran, so `package_effects` does what they would have done.
// The system recipe (`alpine-live-source-rootfs.rhai`) configures the tree
// this publishes. The desktop lock (what the group recipes in recipes/ pin
// beyond the base) is read only for the commands it ships, which BusyBox
// leaves to them.
const NEEDS = #{ inputs: #{ composed: "directory", apks: "directory" }, outputs: #{ tree: "file" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 14400 };
const PACKAGE = #{ name: "alpine-apk-package-effects", description: "The locked Alpine v3.24 APK set with its packages' effects" };
import "sources/acorn-apk-lock.rhai" as lock;
import "sources/acorn-apk-desktop-lock.rhai" as desktop_lock;
import "system-files.rhai" as system;

fn apk_file(row) { join_path(INPUTS.apks, basename(row[2])) }

fn verify_inputs() {
    let apk_tools = lock::apk_tools();
    let files = [[apk_file(apk_tools), apk_tools]];
    for row in lock::packages() { files.push([apk_file(row), row]); }
    for entry in files {
        let file = entry[0];
        if !is_file(file) { throw "the apk inputs lack " + basename(file) + "; run the rootfs source preseed"; }
        verify_sha256(file, entry[1][3]);
    }
    // The composed input is the base tree: the base lock alone.
    verify_composed(lock::packages());
}

fn unpack(root) {
    copy_composed(root);
}

fn package_effects(root) {
    progress(0, 0, "package effects: busybox applets");
    busybox_applets(root, desktop_lock::commands());
    script_accounts(root);
    drop_stock_guest(root);
}

// Alpine's base layout (alpine-baselayout-data) ships a user `guest` (uid
// 405, group users, home /dev/null) that nothing in the base uses: no file is
// owned by it and no group lists it. The login screen's own `guest` account
// (recipes/login-screen.rhai ACCOUNTS: uid and gid 60600, home
// /run/reclogin/guest, as reclogind requires) is only made where no account of
// that name exists, so the stock one goes from the account files.
fn drop_stock_guest(root) {
    for file in [["etc/passwd", 0o644], ["etc/shadow", 0o640], ["etc/group", 0o644], ["etc/gshadow", 0o600]] {
        let path = join_path(root, file[0]);
        if !is_file(path) { continue; }
        let text = "";
        for line in read_file(path).split("\n") {
            if line == "" || line.starts_with("guest:") { continue; }
            let fields = line.split(":");
            // A group file's last field lists members; guest leaves every list.
            if file[0] == "etc/group" || file[0] == "etc/gshadow" {
                let last = fields.len() - 1;
                if last >= 3 && fields[last] != "" {
                    let kept = "";
                    for member in fields[last].split(",") {
                        if member == "guest" { continue; }
                        kept += if kept == "" { member } else { "," + member };
                    }
                    fields[last] = kept;
                    line = fields[0];
                    for i in 1..fields.len() { line += ":" + fields[i]; }
                }
            }
            text += line + "\n";
        }
        system::put(root, file[0], text, file[1]);
    }
}

// ---------------------------------------------------------------------------
// Accounts. apk keeps every installed package's scripts in
// lib/apk/db/scripts.tar.gz even when it runs none. The accounts their
// install scripts would add, with BusyBox's addgroup and adduser, are read
// from there and made in the tree's account files: every group first, then
// every user, then the memberships (`addgroup <user> <group>`). A command in
// a form this does not read stops the build, so no account is missed
// silently.
// ---------------------------------------------------------------------------

fn script_accounts(root) {
    let archive = join_path(root, "lib/apk/db/scripts.tar.gz");
    if !is_file(archive) { throw "apk left no scripts archive at /lib/apk/db/scripts.tar.gz"; }
    let dir = join_path(WORK, "apk-scripts");
    if is_dir(dir) { rm(dir); }
    mkdir(dir);
    extract_with_format(archive, dir, "tar.gz");
    let groups = [];
    let users = [];
    let members = [];
    let names = shell_output("ls '" + dir + "'").split("\n");
    let seen = 0;
    for name in names {
        seen += 1;
        progress(seen, names.len(), "package scripts read");
        let name = trim(name);
        if !(name.ends_with(".pre-install") || name.ends_with(".post-install")) { continue; }
        for command in account_commands(read_file(join_path(dir, name))) {
            let where = name + ": " + command;
            let words = command.split(" ");
            if words[0] == "addgroup" {
                let group = addgroup_args(words, where);
                if group.member != "" { members.push(group); } else { groups.push(group); }
            } else {
                let user = adduser_args(words, where);
                users.push(user);
                for group in user.groups { members.push(#{ member: user.name, name: group }); }
            }
        }
    }
    let accounts = groups.len() + users.len() + members.len();
    let made = 0;
    progress(0, accounts, "accounts");
    for group in groups { add_group(root, group.name, group.id); made += 1; progress(made, accounts, "accounts"); }
    for user in users { add_user(root, user); made += 1; progress(made, accounts, "accounts"); }
    for group in members { add_member(root, group.member, group.name); made += 1; progress(made, accounts, "accounts"); }
}

// The addgroup and adduser commands of a shell script's text, each as its
// words joined by single spaces: lines continued with a backslash are
// joined, comments are skipped, and a command ends at a redirection, `;`,
// `|`, `||` or `&&`.
fn account_commands(text) {
    let commands = [];
    let line = "";
    for part in text.split("\n") {
        if part.ends_with("\\") { line += part.sub_string(0, part.len() - 1) + " "; continue; }
        line += part;
        let words = shell_words(trim(line));
        line = "";
        if words.is_empty() || words[0].starts_with("#") { continue; }
        let start = -1;
        for i in 0..words.len() {
            if words[i] == "addgroup" || words[i] == "adduser" { start = i; break; }
        }
        if start < 0 { continue; }
        let command = "";
        for i in start..words.len() {
            let word = words[i];
            if word == ";" || word == "|" || word == "||" || word == "&&" || word.starts_with(">") || word.starts_with("2>") || word.starts_with("&>") { break; }
            if word.ends_with(";") {
                command += " " + word.sub_string(0, word.len() - 1);
                break;
            }
            command += " " + word;
        }
        commands.push(trim(command));
    }
    commands
}

// A shell line's words, with single and double quotes removed. A space
// inside quotes becomes "_" so the word stays whole (only a GECOS holds one;
// adduser_args turns it back).
fn shell_words(line) {
    let words = [];
    let word = "";
    let quote = "";
    let started = false;
    for c in line.chars() {
        let ch = "" + c;
        if quote != "" {
            if ch == quote { quote = ""; } else if ch == " " { word += "_"; } else { word += ch; }
        } else if ch == "\"" || ch == "'" {
            quote = ch;
            started = true;
        } else if ch == " " || ch == "\t" {
            if started { words.push(word); word = ""; started = false; }
        } else {
            word += ch;
            started = true;
        }
    }
    if started { words.push(word); }
    words
}

// `addgroup [-S] [-g GID] GROUP` (a group) or `addgroup USER GROUP` (a
// membership), as #{name, id, member}; id -1 is any free id.
fn addgroup_args(words, where) {
    let group = #{ name: "", id: -1, member: "" };
    let names = [];
    let i = 1;
    while i < words.len() {
        let word = words[i];
        if word == "-S" { }
        else if word == "-g" { i += 1; group.id = parse_int(words[i]); }
        else if word.starts_with("-") || word.contains("$") { throw "an addgroup this recipe does not read (" + where + ")"; }
        else { names.push(word); }
        i += 1;
    }
    if names.len() == 1 { group.name = names[0]; }
    else if names.len() == 2 { group.member = names[0]; group.name = names[1]; }
    else { throw "an addgroup this recipe does not read (" + where + ")"; }
    group
}

// `adduser -S [-D] [-H] [-h HOME] [-s SHELL] [-G GROUP] [-g GECOS] [-u UID]
// USER [GROUP]`, as #{name, id, group, home, shell, gecos, groups}. Without
// -G, BusyBox gives the user a group of its own name.
fn adduser_args(words, where) {
    let user = #{ name: "", id: -1, group: "", home: "", shell: "/sbin/nologin", gecos: "", groups: [] };
    let names = [];
    let i = 1;
    while i < words.len() {
        let word = words[i];
        let value = if i + 1 < words.len() { words[i + 1] } else { "" };
        if word == "-h" { user.home = value; i += 1; }
        else if word == "-s" { user.shell = value; i += 1; }
        else if word == "-G" { user.group = value; i += 1; }
        else if word == "-g" {
            // Recipe's replace returns the new string (it does not edit in place).
            user.gecos = value.replace("_", " ");
            i += 1;
        }
        else if word == "-u" { user.id = parse_int(value); i += 1; }
        else if boolean_flags(word, "SDH") { }
        else if word.starts_with("-") || word.contains("$") { throw "an adduser this recipe does not read (" + where + ")"; }
        else { names.push(word); }
        i += 1;
    }
    if names.is_empty() || names.len() > 2 { throw "an adduser this recipe does not read (" + where + ")"; }
    user.name = names[0];
    if names.len() == 2 { user.groups.push(names[1]); }
    if user.home == "" { user.home = "/home/" + user.name; }
    if user.gecos == "" { user.gecos = user.name; }
    user
}

// Whether `word` is one or more of the single-letter flags `letters`
// ("-S", "-SDH").
fn boolean_flags(word, letters) {
    if !word.starts_with("-") || word.len() < 2 { return false; }
    for c in word.sub_string(1).chars() {
        if !letters.contains(c) { return false; }
    }
    true
}

fn has_entry(root, file, name) {
    read_file(join_path(root, file)).split("\n").some(|line| line.starts_with(name + ":"))
}
// Whether `id` is taken in the account file `file` (etc/passwd or etc/group).
fn id_used(root, file, id) {
    read_file(join_path(root, file)).split("\n")
        .some(|line| line.split(":").len() >= 3 && parse_int(line.split(":")[2]) == id)
}
// The lowest id free in both account files (addgroup -S and adduser -S
// start at 100).
fn free_id(root) {
    let id = 100;
    while id_used(root, "etc/passwd", id) || id_used(root, "etc/group", id) { id += 1; }
    id
}
fn append(root, file, line, mode) {
    let text = read_file_or_empty(join_path(root, file));
    if text != "" && !text.ends_with("\n") { text += "\n"; }
    system::put(root, file, text + line + "\n", mode);
}
fn group_id(root, name) {
    for line in read_file(join_path(root, "etc/group")).split("\n") {
        if line.starts_with(name + ":") { return parse_int(line.split(":")[2]); }
    }
    throw "the rootfs has no group " + name;
}

// The group `name` with id `id` where it is free (-1: any free id).
fn add_group(root, name, id) {
    if has_entry(root, "etc/group", name) { return; }
    let gid = if id >= 0 && !id_used(root, "etc/group", id) { id } else { free_id(root) };
    append(root, "etc/group", name + ":x:" + gid + ":", 0o644);
}

// The user `user` (adduser_args), with a locked password; a user whose
// group has its own name takes the group's id where it is free.
fn add_user(root, user) {
    if has_entry(root, "etc/passwd", user.name) { return; }
    let group = user.group;
    if group == "" {
        group = user.name;
        add_group(root, group, user.id);
    }
    let gid = group_id(root, group);
    let uid = if user.id >= 0 && !id_used(root, "etc/passwd", user.id) { user.id }
        else if group == user.name && !id_used(root, "etc/passwd", gid) { gid }
        else { free_id(root) };
    append(root, "etc/passwd", user.name + ":x:" + uid + ":" + gid + ":" + user.gecos + ":" + user.home + ":" + user.shell, 0o644);
    append(root, "etc/shadow", trim(system::shadow_line(user.name, "!")), 0o640);
}

// `member` in the supplementary members of the group `name`.
fn add_member(root, member, name) {
    let text = "";
    let found = false;
    for line in read_file(join_path(root, "etc/group")).split("\n") {
        if line == "" { continue; }
        let fields = line.split(":");
        if fields.len() == 4 && fields[0] == name {
            found = true;
            let current = fields[3].split(",");
            if !current.contains(member) {
                line += if fields[3] == "" { member } else { "," + member };
            }
        }
        text += line + "\n";
    }
    if !found { throw "an install script adds " + member + " to the missing group " + name; }
    system::put(root, "etc/group", text, 0o644);
}

// BusyBox's applet links, from its own packaged inventory; files the
// full-size tools ship stay, and no link is made for a command in `taken`
// (what the desktop lock's packages ship, by their `cmd:` provides): a group
// recipe that installs the full-size tool (bc, unzip, partprobe, ...) must
// not find a base file in its place.
fn busybox_applets(root, taken) {
    let privileged = ["bin/mount", "bin/umount", "bin/su", "usr/bin/crontab",
        "usr/bin/passwd", "usr/bin/traceroute", "usr/bin/traceroute6", "usr/bin/vlock"];
    for path in read_file(join_path(root, "etc/busybox-paths.d/busybox")).split("\n") {
        path = trim(path);
        if path == "" { continue; }
        if !(path.starts_with("bin/") || path.starts_with("sbin/") ||
             path.starts_with("usr/bin/") || path.starts_with("usr/sbin/")) ||
             path.contains("..") {
            throw "invalid packaged BusyBox applet path: " + path;
        }
        let dest = join_path(root, path);
        if exists(dest) || taken.contains(basename(path)) { continue; }
        let program = if privileged.contains(path) { "bbsuid" } else { "busybox" };
        if !is_file(join_path(root, "bin/" + program)) { throw "missing BusyBox provider: " + program; }
        mkdir(dirname(dest));
        let prefix = if path.starts_with("usr/") { "../../bin/" }
                     else if path.starts_with("sbin/") { "../bin/" } else { "" };
        ln_force(prefix + program, dest);
    }
}
