//! extends: package-effects.rhai
// The RPM backend of the package-effects step: the composed tree of the
// locked Fedora RPM set (base, and on the desktop system the desktop lock
// over it, as the `flavour` input says), whose install scripts never ran, so
// `package_effects` does what they would have done, from the packages' own
// data where they ship it. The system recipe (`fedora-rpm-source-rootfs.rhai`)
// configures the tree this publishes.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ composed: "directory", flavour: "file" }, outputs: #{ tree: "file" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 14400 };
import "sources/levitate-rpm-lock.rhai" as lock;
import "sources/levitate-rpm-desktop-lock.rhai" as desktop_lock;
import "system-files.rhai" as system;
let ctx = #{
    name: "fedora-rpm-package-effects",
    description: "The locked Fedora " + lock::release() + " RPM set with its packages' effects",
};

// The lock rows the tree is made of: the base lock, and the desktop lock on
// the desktop system.
fn lock_rows(ctx) {
    let rows = lock::packages();
    if flavour(ctx) == "desktop" { rows += desktop_lock::packages(); }
    rows
}

fn verify_inputs(ctx) {
    verify_composed(ctx, lock_rows(ctx));
}

fn unpack(ctx, root) {
    log("Copying the composed Fedora " + lock::release() + " RPM set into the tree...");
    copy_composed(ctx, root);
}

fn package_effects(ctx, root) {
    progress(0, 0, "package effects");
    // `filesystem` lists /usr/sbin as a ghost that its script makes a link to
    // bin; the merged-/usr links at the root are in its payload.
    let sbin = join_path(root, "usr/sbin");
    if !exists(sbin) { ln_force("bin", sbin); }
    if shell_status("[ \"$(readlink '" + sbin + "')\" = bin ]") != 0 {
        throw "the locked filesystem package ships /usr/sbin, not the ghost link to bin";
    }
    for link in [["bin", "usr/bin"], ["sbin", "usr/sbin"], ["lib", "usr/lib"], ["lib64", "usr/lib64"]] {
        if shell_status("[ \"$(readlink '" + join_path(root, link[0]) + "')\" = " + link[1] + " ]") != 0 {
            throw "the locked filesystem package lacks the merged-/usr link /" + link[0];
        }
    }
    // The kernel filesystems' mount points. systemd's switch-root moves /proc
    // and /sys into the new root and gives up without them, so a slot recab
    // stages from this image must carry them.
    for dir in ["proc", "sys"] { system::directory(root, dir, 0o555); }
    progress(0, 0, "package effects: accounts (sysusers)");
    sysusers(root);
    // dbus-broker's preset: it is the system bus, started from its socket.
    system::link(root, "etc/systemd/system/dbus.service", "/usr/lib/systemd/system/dbus-broker.service");
    system::link(root, "etc/systemd/system/sockets.target.wants/dbus.socket", "/usr/lib/systemd/system/dbus.socket");
    // The PAM stack authselect would generate: systemd's factory system-auth,
    // and password-auth and postlogin over it.
    let factory = join_path(root, "usr/share/factory/etc/pam.d/system-auth");
    if !is_file(factory) { throw "the locked systemd lacks its factory PAM system-auth"; }
    system::put(root, "etc/pam.d/system-auth", read_file(factory), 0o644);
    system::put(root, "etc/pam.d/password-auth",
        "#%PAM-1.0\nauth        include      system-auth\naccount     include      system-auth\n" +
        "password    include      system-auth\nsession     include      system-auth\n", 0o644);
    system::put(root, "etc/pam.d/postlogin", "#%PAM-1.0\nsession     optional     pam_keyinit.so force revoke\n", 0o644);
}

// The words of a sysusers.d line; '…' and "…" quote one word.
fn words(line) {
    let out = [];
    let word = "";
    let quote = "";
    let open = false;
    for ch in line.chars() {
        let c = "" + ch;
        if quote != "" {
            if c == quote { quote = ""; } else { word += c; }
        } else if c == "\"" || c == "'" {
            quote = c; open = true;
        } else if c == " " || c == "\t" {
            if open || word != "" { out.push(word); word = ""; open = false; }
        } else {
            word += c; open = true;
        }
    }
    if open || word != "" { out.push(word); }
    out
}

// The ids in a passwd or group file, by name.
fn ids(text) {
    let map = #{};
    for line in text.split("\n") {
        let fields = line.split(":");
        if fields.len() >= 3 { map[fields[0]] = parse_int(fields[2]); }
    }
    map
}

// The accounts the packages declare in usr/lib/sysusers.d, made as
// systemd-sysusers makes them (accounts already there are kept; an id of "-"
// is the highest free system id). target_finalize resolves the packages' file
// owners against them, and sshd and dbus need theirs before the first boot.
fn sysusers(root) {
    let text = |file| {
        let t = read_file_or_empty(join_path(root, file));
        if t != "" && !t.ends_with("\n") { t += "\n"; }
        t
    };
    let passwd = text.call("etc/passwd");
    let group = text.call("etc/group");
    let shadow = text.call("etc/shadow");
    let gshadow = text.call("etc/gshadow");
    let users = ids(passwd);
    let groups = ids(group);
    let free = |used| { let id = 999; while used.values().contains(id) { id -= 1; } id };
    let dir = join_path(root, "usr/lib/sysusers.d");
    let lines = [];
    for file in shell_output("ls '" + dir + "'").split("\n") {
        if file.ends_with(".conf") { lines += read_file(join_path(dir, file)).split("\n"); }
    }
    for kind in ["g", "u"] {
        for line in lines {
            let w = words(trim(line));
            if w.len() < 2 || w[0] != kind { continue; }
            let name = w[1];
            let id = if w.len() > 2 { w[2] } else { "-" };
            if kind == "g" {
                if name in groups { continue; }
                let gid = if id == "-" { free.call(groups) } else { parse_int(id) };
                groups[name] = gid;
                group += name + ":x:" + gid + ":\n";
                gshadow += name + ":!::\n";
                continue;
            }
            if name in users { continue; }
            let uid = 0;
            let gid = 0;
            if id.contains(":") {
                let parts = id.split(":");
                uid = parse_int(parts[0]);
                gid = if parts[1] in groups { groups[parts[1]] } else { parse_int(parts[1]) };
            } else {
                uid = if id == "-" { free.call(users + groups) } else { parse_int(id) };
                if name in groups { gid = groups[name]; }
                else {
                    gid = if groups.values().contains(uid) { free.call(groups) } else { uid };
                    groups[name] = gid;
                    group += name + ":x:" + gid + ":\n";
                    gshadow += name + ":!::\n";
                }
            }
            let gecos = if w.len() > 3 && w[3] != "-" { w[3] } else { "" };
            let home = if w.len() > 4 && w[4] != "-" { w[4] } else { "/" };
            let shell = if w.len() > 5 && w[5] != "-" { w[5] } else { "/usr/sbin/nologin" };
            users[name] = uid;
            passwd += name + ":x:" + uid + ":" + gid + ":" + gecos + ":" + home + ":" + shell + "\n";
            shadow += system::shadow_line(name, "!*");
        }
    }
    system::put(root, "etc/passwd", passwd, 0o644);
    system::put(root, "etc/group", group, 0o644);
    system::put(root, "etc/shadow", shadow, 0o600);
    system::put(root, "etc/gshadow", gshadow, 0o600);
}
