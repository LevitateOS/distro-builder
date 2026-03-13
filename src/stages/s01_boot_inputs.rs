pub use crate::pipeline::products::{
    load_base_rootfs_product_spec as load_s00_build_input_spec,
    load_live_boot_product_spec as load_s01_boot_input_spec,
    materialize_live_boot_source_rootfs as materialize_s01_source_rootfs,
    prepare_base_rootfs_product as prepare_s00_build_inputs,
    prepare_live_boot_product as prepare_s01_boot_inputs, BaseRootfsProduct as S00BuildInputs,
    BaseRootfsProductSpec as S00BuildInputSpec, LiveBootProduct as S01BootInputs,
    LiveBootProductSpec as S01BootInputSpec,
};
