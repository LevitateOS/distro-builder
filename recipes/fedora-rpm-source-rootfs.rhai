//! extends: rootfs-producer.rhai
// The RPM backend of the rootfs producer: the locked Fedora RPM set (the
// `rpms` input), unpacked without a package manager. Their install scripts
// do not run, so `package_effects` does what they would have done, from the
// packages' own data where they ship it.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ rpms: "directory", capabilities: "file", system_setup: "file", os_release: "file" }, outputs: #{ rootfs: "directory" }, memory_mib: 8192, storage_max_objects: 131072 };
import "sources/levitate-rpm-lock.rhai" as lock;
import "installed-system.rhai" as system;
let ctx = #{
    name: "fedora-rpm-source-rootfs",
    description: "The system rootfs from the locked Fedora " + lock::release() + " RPM set",
    init: "systemd",
    rpms_path: join_path(RUNTIME.paths.inputs, "rpms"),
    packages: lock::packages(),
};

fn rpm_file(ctx, rpm) { join_path(ctx.rpms_path, basename(rpm[2])) }

fn verify_inputs(ctx) {
    for rpm in ctx.packages {
        let file = rpm_file(ctx, rpm);
        if !is_file(file) { throw "the rpms input lacks " + basename(file) + "; run the rootfs source preseed"; }
        verify_sha256(file, rpm[3]);
    }
}

fn unpack(ctx, root) {
    log("Extracting the locked Fedora " + lock::release() + " RPM set into the rootfs...");
    for rpm in ctx.packages {
        log("  Extracting " + rpm[1]);
        target_extract_rpm(rpm_file(ctx, rpm), root);
    }
}

fn package_effects(ctx, root) {
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
    // Password-authenticated sudo for wheel administrators.
    system::put(root, "etc/sudoers.d/10-wheel", "%wheel ALL=(ALL:ALL) ALL\n", 0o440);
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
