// Shared pinned package facts; no acquisition side effects.
fn facts() {
    let iso_version = "3.23.2";
    let apk_tools_version = "3.0.9-r0";
    let release_branch = "v3.23";
    let iso_name = "alpine-extended-" + iso_version + "-x86_64.iso";
    let apk_tools_name = "apk-tools-static-" + apk_tools_version + ".apk";
    let mirror = "https://dl-cdn.alpinelinux.org/alpine/" + release_branch;
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
    }
}
