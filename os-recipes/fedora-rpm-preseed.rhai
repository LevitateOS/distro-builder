//! extends: locked-preseed.rhai
// The locked Fedora RPM set, which the rootfs recipe imports as `rpms`.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ target: "directory?" }, outputs: #{ rpms: "directory" }, network: ["dl.fedoraproject.org:443"], memory_mib: 8192 };
import "sources/levitate-rpm-lock.rhai" as lock;
let ctx = #{
    name: "fedora-rpm-preseed",
    description: "Locked Fedora " + lock::release() + " RPM set",
    output: "rpms",
    files: lock::packages(),
};
