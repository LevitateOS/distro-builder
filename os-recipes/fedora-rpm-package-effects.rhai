//! extends: package-effects.rhai
// The RPM backend of the package-effects step: the composed tree of the
// base lock's Fedora RPMs, whose install scripts never ran, so
// `package_effects` does what they would have done, from the packages' own
// data where they ship it. The system recipe (`fedora-rpm-source-rootfs.rhai`)
// configures the tree this publishes.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ composed: "directory" }, outputs: #{ tree: "file" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 14400 };
import "sources/levitate-rpm-lock.rhai" as lock;
import "system-files.rhai" as system;
let ctx = #{
    name: "fedora-rpm-package-effects",
    description: "The locked Fedora " + lock::release() + " RPM set with its packages' effects",
};

fn verify_inputs(ctx) {
    verify_composed(ctx, lock::packages());
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
    progress(0, 0, "package effects: CA trust (update-ca-trust)");
    ca_trust(root);
}

// What ca-certificates' scriptlet and file trigger do: `update-ca-trust
// extract`, which turns the package's p11-kit trust source
// (usr/share/pki/ca-trust-source) into everything under
// etc/pki/ca-trust/extracted (the PEM bundles /etc/ssl/certs/* point at, the
// OpenSSL trust bundle, the EDK2 and Java stores, the directory-hash set) and
// the hash links in etc/pki/tls/certs. The RPM payload holds only the source
// and the tool, so without this every native-TLS client (Recipe's rustls
// included) finds a dangling /etc/ssl/certs/ca-certificates.crt.
//
// The tree's own `trust` runs through the tree's own loader and libraries
// (as `tree_run` in installed-system.rhai does), and issues the calls of the
// tree's update-ca-trust script. p11-kit reads its module configuration and
// trust source from absolute paths, which in the build domain are the
// image's, not the tree's; the one documented redirect is a user module
// config (XDG_CONFIG_HOME/pkcs11/modules/p11-kit-trust.module replaces the
// system module of that name, with `paths=` for the source), and p11-kit
// ignores user configuration for uid 0, which is what the domain's workload
// is. The domain has no unshare, chroot or setpriv, but setuid works, so a
// two-line python (the image's) drops to an ordinary uid and execs the
// loader. SOURCE_DATE_EPOCH pins the one non-deterministic part of the
// output (the creation dates in the Java keystore).
fn ca_trust_shim() { "import os,sys; os.setgid(65534); os.setuid(65534); os.execv(sys.argv[1], sys.argv[1:])" }

fn ca_trust(root) {
    let source = "usr/share/pki/ca-trust-source";
    let sources = join_path(root, "etc/pki/ca-trust/source") + ":" + join_path(root, source);
    for file in [source + "/ca-bundle.trust.p11-kit", "usr/bin/trust", "usr/bin/update-ca-trust",
                 "usr/lib64/pkcs11/p11-kit-trust.so", "usr/lib64/ld-linux-x86-64.so.2"] {
        if !is_file(join_path(root, file)) { throw "the locked ca-certificates and p11-kit-trust lack /" + file; }
    }
    // The calls update-ca-trust makes (destination names are under
    // extracted/); the script must still make exactly these.
    let calls = [
        ["--format=openssl-bundle", "--filter=certificates", "--overwrite", "--comment", "openssl/ca-bundle.trust.crt"],
        ["--format=pem-bundle", "--filter=ca-anchors", "--overwrite", "--comment", "--purpose", "server-auth", "pem/tls-ca-bundle.pem"],
        ["--format=pem-bundle", "--filter=ca-anchors", "--overwrite", "--comment", "--purpose", "email", "pem/email-ca-bundle.pem"],
        ["--format=pem-bundle", "--filter=ca-anchors", "--overwrite", "--comment", "--purpose", "code-signing", "pem/objsign-ca-bundle.pem"],
        ["--format=java-cacerts", "--filter=ca-anchors", "--overwrite", "--purpose", "server-auth", "java/cacerts"],
        ["--format=edk2-cacerts", "--filter=ca-anchors", "--overwrite", "--purpose=server-auth", "edk2/cacerts.bin"],
        ["--format=pem-directory-hash", "--filter=ca-anchors", "--overwrite", "--purpose", "server-auth", "pem/directory-hash"],
    ];
    let script = read_file(join_path(root, "usr/bin/update-ca-trust"));
    for entry in calls {
        let line = "/usr/bin/trust extract";
        for i in 0..entry.len() - 1 { line += " " + entry[i]; }
        line += " \"$DEST/" + entry[entry.len() - 1] + "\"";
        if !script.contains(line) { throw "the locked update-ca-trust no longer runs `" + line + "`"; }
    }
    // A scratch directory the unprivileged uid can write, and the user
    // module config that points p11-kit at the tree.
    let work = join_path(dirname(root), "ca-trust.work");
    if exists(work) { rm(work); }
    mkdir(work);
    chmod(work, 0o777);
    let cfg = join_path(work, "config/pkcs11/modules");
    mkdir(join_path(work, "config"));
    mkdir(join_path(work, "config/pkcs11"));
    mkdir(cfg);
    write_file(join_path(cfg, "p11-kit-trust.module"),
        "module: " + join_path(root, "usr/lib64/pkcs11/p11-kit-trust.so") + "\npriority: 1\ntrust-policy: yes\n" +
        "x-trust-lookup: pkcs11:library-description=PKCS%2311%20Kit%20Trust%20Module\ndisable-in: p11-kit-proxy\n" +
        "x-init-reserved: paths=" + sources + "\n");
    chmod(join_path(work, "config"), 0o755);
    let out = join_path(work, "out");
    for dir in ["", "/openssl", "/pem", "/java", "/edk2"] { mkdir(out + dir); chmod(out + dir, 0o777); }
    let loader = join_path(root, "usr/lib64/ld-linux-x86-64.so.2");
    chmod(loader, 0o755);
    for entry in calls {
        let args = [ca_trust_shim(), loader, "--library-path", join_path(root, "usr/lib64"),
                    join_path(root, "usr/bin/trust"), "extract"];
        for i in 0..entry.len() - 1 { args.push(entry[i]); }
        args.push(join_path(out, entry[entry.len() - 1]));
        exec_checked("python3", ["-c"] + args,
            #{ env: #{ XDG_CONFIG_HOME: join_path(work, "config"), HOME: work, SOURCE_DATE_EPOCH: "1" } });
    }
    // What the script does with the results: the files into extracted/, the
    // hash links (made absolute, as `readlink -f` makes them) into
    // etc/pki/tls/certs.
    let extracted = join_path(root, "etc/pki/ca-trust/extracted");
    for entry in calls {
        let name = entry[entry.len() - 1];
        let dest = join_path(extracted, name);
        if !exists(join_path(out, name)) { throw "trust extract made no " + name; }
        if is_dir(join_path(out, name)) {
            if is_dir(dest) { rm(dest); }
            shell("cp -R '" + join_path(out, name) + "' '" + dest + "'");
        } else {
            if exists(dest) { rm(dest); }
            copy_file(join_path(out, name), dest);
        }
    }
    let hashes = join_path(extracted, "pem/directory-hash");
    let certs = join_path(root, "etc/pki/tls/certs");
    shell("find '" + certs + "' -maxdepth 1 -type l -regextype posix-extended -regex '.*/[0-9a-f]{8}\\.[0-9]+' -delete");
    shell("cd '" + hashes + "' && for l in $(find . -maxdepth 1 -type l -regextype posix-extended -regex '.*/[0-9a-f]{8}\\.[0-9]+' | sort); do " +
          "ln -s \"/etc/pki/ca-trust/extracted/pem/directory-hash/$(readlink \"$l\")\" '" + certs + "'/$(basename \"$l\"); done");
    rm(work);
    let bundle = join_path(extracted, "pem/tls-ca-bundle.pem");
    if shell_status("grep -q 'BEGIN CERTIFICATE' '" + bundle + "'") != 0 {
        throw "trust extract made an empty TLS bundle: the trust source holds no server-auth roots";
    }
    if trim(shell_output("ls '" + certs + "'")) == "" { throw "trust extract made no hash links in /etc/pki/tls/certs"; }
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
