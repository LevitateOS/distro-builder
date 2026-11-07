// Shared pinned package facts; no acquisition side effects.
fn facts() {
    let iso_version = "3.23.2";
    let apk_tools_version = "3.0.9-r0";
    let release_branch = "v3.23";
    let iso_name = "alpine-extended-" + iso_version + "-x86_64.iso";
    let apk_tools_name = "apk-tools-static-" + apk_tools_version + ".apk";
    let mirror = "https://dl-cdn.alpinelinux.org/alpine/" + release_branch;
    // Packages the AcornOS rootfs installs that the ISO does not carry (their
    // dependencies are on it). The rootfs build has no network, so the preseed
    // fetches these, pinned like apk-tools, and the rootfs installs the files.
    let extra_apks = [
        ["findutils-4.10.0-r0.apk", "e6a6642b2a2d58f772dccea4ab598d14d76a17c8f2917851e06a3bed13c90421"],
        ["grep-3.12-r0.apk", "e7373b70fbefc2712ec385b36adec5ad45aedab23b5471fd1959df4e906bbfdd"],
        ["hdparm-9.65-r2.apk", "2417335fe9d3ae7b6dcb5cd54877b86a1f894e6f74719168cade2e23cb0d1835"],
        ["less-685-r0.apk", "b7a451c8671ac6b659fa45e0f2b6bb4d18a416f72c8443af459a6928e56ed605"],
        ["musl-locales-0.1.0-r1.apk", "0a169d6805d7c2d86ebfaddb73be0883d962ce0fc6b45f4eddfa79ffb2d48eb4"],
        ["musl-locales-lang-0.1.0-r1.apk", "f3fe0baa9c9f622510945af09bbd373a547d145cf5a0461cc4931672dd3e0659"],
        ["openrc-init-0.63-r1.apk", "c0f27be12aaccbda2222d62e0b06ad6722e05bb9045a73d1616b7673351c6667"],
        ["smartmontools-7.5-r0.apk", "5121e1979bb9938b3e5966bf7da1150fd3f44f8cff2ed263172af20725fd9ad7"],
        ["systemd-boot-258.3-r0.apk", "df4df7a6411112526a8f5f1fc42706db1e9545ab502f6ec35d1d026246c91583"],
    ];
    #{
        iso_version: iso_version,
        apk_tools_version: apk_tools_version,
        release_branch: release_branch,
        iso_name: iso_name,
        apk_tools_name: apk_tools_name,
        iso_url: mirror + "/releases/x86_64/" + iso_name,
        apk_tools_url: mirror + "/main/x86_64/" + apk_tools_name,
        iso_sha256: "8d50854936dba58e7616ac3bfa07ad67dac0347305fec1cba6d288cf5df1577d",
        // Full APK pin authenticated against Alpine index/control/payload RSA signatures.
        apk_tools_sha256: "ea77b2533566e70c9566c03748199b79348010c3199ba02d97e0634cf0fa9ef7",
        // [file name, sha256]; each is fetched from extra_apks_url + file name.
        extra_apks: extra_apks,
        extra_apks_url: mirror + "/main/x86_64/",
    }
}
