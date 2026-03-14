use std::path::Path;

use anyhow::Result;

pub(crate) fn dispatch_compatibility_command(args: &[String]) -> Option<Result<()>> {
    match args {
        [artifact, build_stage_erofs, stage, distro]
            if artifact == "artifact" && build_stage_erofs == "build-stage-erofs" =>
        {
            Some(crate::workflows::build_stage_erofs_cmd(stage, distro))
        }
        [artifact, prepare_stage, stage, distro, output_dir]
            if artifact == "artifact" && prepare_stage == "prepare-stage-inputs" =>
        {
            Some(crate::workflows::prepare_stage_inputs_cmd(
                stage,
                distro,
                Path::new(output_dir),
            ))
        }
        [artifact, prepare_s01, distro, output_dir]
            if artifact == "artifact" && prepare_s01 == "prepare-s01-boot-inputs" =>
        {
            Some(crate::workflows::prepare_stage_inputs_cmd(
                "01Boot",
                distro,
                Path::new(output_dir),
            ))
        }
        [artifact, prepare_s02, distro, output_dir]
            if artifact == "artifact" && prepare_s02 == "prepare-s02-live-tools-inputs" =>
        {
            Some(crate::workflows::prepare_stage_inputs_cmd(
                "02LiveTools",
                distro,
                Path::new(output_dir),
            ))
        }
        [artifact, prepare_s00, distro, output_dir]
            if artifact == "artifact" && prepare_s00 == "prepare-s00-build-inputs" =>
        {
            Some(crate::workflows::prepare_stage_inputs_cmd(
                "00Build",
                distro,
                Path::new(output_dir),
            ))
        }
        _ => None,
    }
}
