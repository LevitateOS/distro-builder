//! extends: locked-preseed.rhai
// The locked Fedora RPMs the desktop adds to the base (the desktop lock),
// which the rootfs recipe imports as `desktop_rpms` when it builds the
// desktop system.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ target: "directory?", store: "directory?" }, outputs: #{ desktop_rpms: "directory" }, network: ["dl.fedoraproject.org:443"], memory_mib: 8192 };
import "sources/levitate-rpm-desktop-lock.rhai" as lock;
let ctx = #{
    name: "fedora-rpm-desktop-preseed",
    description: "Locked Fedora " + lock::release() + " desktop RPM set",
    output: "desktop_rpms",
    files: lock::packages(),
};
