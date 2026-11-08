// Shared pinned package facts; no acquisition side effects.
fn facts() {
    let iso_version = "3.24.4";
    let apk_tools_version = "3.0.9-r0";
    let release_branch = "v3.24";
    let iso_name = "alpine-extended-" + iso_version + "-x86_64.iso";
    let apk_tools_name = "apk-tools-static-" + apk_tools_version + ".apk";
    let mirror = "https://dl-cdn.alpinelinux.org/alpine/" + release_branch;
    // Packages the AcornOS rootfs installs that the ISO does not carry (their
    // other dependencies are on it). The rootfs build has no network, so the
    // preseed fetches these, pinned like apk-tools, and the rootfs installs the
    // files. Each was checked against Alpine's package signing keys from the
    // GPG-verified ISO. [file name, sha256, repository]; each is fetched from
    // extra_apks_url + repository + "/x86_64/" + file name.
    let extra_apks = [
        ["findutils-4.10.0-r1.apk", "86cf2b3f8aa3b1092cb3225fcc8179fc1229d178aa126266cfeadfda09487dc2", "main"],
        ["grep-3.12-r0.apk", "a39238e2ddadc483c1512145bac8e246af940d6c828e5e14fb99a287eb139f42", "main"],
        ["hdparm-9.65-r2.apk", "43fbe9f0016efd8f58af171ae3f96282e3d614fee1cb2fd6661fb9a2c9df152c", "main"],
        ["less-702-r0.apk", "afa47eee1729eab04d6d72142c52dea513c756b71be91cc13756fa628282ec7e", "main"],
        ["musl-locales-0.1.0-r1.apk", "384a7e572640c5d3dc6ac82cdf39a67d3ffbf2fbad793437e68b7c6fb798d046", "main"],
        ["musl-locales-lang-0.1.0-r1.apk", "7f958b1a9decf09207b0c16cc43c78254099092265fcc8887ec02631a8ab0683", "main"],
        ["openrc-init-0.63.2-r0.apk", "1d01e1216092e170879db2fd9fef4f924c27368c048ab40d9a8a51205c547662", "main"],
        ["pixman-0.46.4-r0.apk", "378550cbe348dda9b0733014375dba87a89f0077c5f3dbeba7dce8dbcdda707e", "main"],
        ["smartmontools-7.5-r0.apk", "7e7c45c02d2f3d358711cad718bc6d6c0bc84f723f2c8e8b5876acf1d6c09e73", "main"],
        ["systemd-boot-260.2-r0.apk", "eeef3a0919785d1993dc699f2ca455ad1a84dded86050d7ddfdabdaf954406ce", "main"],
        // vim moved from main to community in 3.24, and left the ISO.
        ["vim-9.2.1202-r0.apk", "922cdfce26e19a6413bbbd7110b7aa9460c7445caca41cf433341b243ad6cece", "community"],
        ["vim-common-9.2.1202-r0.apk", "1565a62139d4049e7ade4fbbf58964d99ec7f33b78a8328061b426364ca2c251", "community"],
        ["xxd-9.2.1202-r0.apk", "22e8c267798e6b874b3efad6dee73048a5eef9ef993955af4ab4489186ceae8a", "community"],
    ];
    #{
        iso_version: iso_version,
        apk_tools_version: apk_tools_version,
        release_branch: release_branch,
        iso_name: iso_name,
        apk_tools_name: apk_tools_name,
        iso_url: mirror + "/releases/x86_64/" + iso_name,
        apk_tools_url: mirror + "/main/x86_64/" + apk_tools_name,
        // Alpine's published sha256; the ISO's .asc verifies with Natanael
        // Copa's release key 0482D84022F52DF1C4E7CD43293ACD0907D9495A.
        iso_sha256: "e14670256c008398f06e985ffa5f2844f4ecc1944b4a004f9dfc1b87265586ce",
        // Full APK pin authenticated against Alpine index/control/payload RSA signatures.
        apk_tools_sha256: "0a1a0682375f66a83b7b903f6e4638ccfe312547d50d336b5f2be9fc391281be",
        extra_apks: extra_apks,
        extra_apks_url: mirror + "/",
        // The release branch's repositories, which /etc/apk/repositories names.
        mirror: mirror,
    }
}
