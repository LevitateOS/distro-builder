//! extends: locked-preseed.rhai
// The locked Fedora RPM set, which the rootfs recipe imports as `rpms`.
const RECIPE_API = 2;
import "sources/levitate-rpm-lock.rhai" as lock;
let ctx = #{
    name: "fedora-rpm-preseed",
    description: "Locked Fedora " + lock::release() + " RPM set",
    output: "rpms",
    files: lock::packages(),
};
