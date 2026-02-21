use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

pub(crate) fn parse_build_command(
    args: Vec<&String>,
    repo_root: &Path,
) -> Result<(String, crate::BuildStage)> {
    let known_distros = crate::workflows::discover_distro_ids(repo_root)?;

    match args.as_slice() {
        [] => Ok((crate::DEFAULT_DISTRO_ID.to_string(), parse_stage(None)?)),
        [arg] => parse_build_one_arg(arg, &known_distros),
        [arg1, arg2] => parse_build_two_args(arg1, arg2, &known_distros),
        _ => bail!(
            "unsupported positional arguments for `iso build`; expected `[stage_or_distro] [stage_or_distro]`, \
             max 2 args"
        ),
    }
}

pub(crate) fn parse_build_one_arg(
    arg: &str,
    known_distros: &[String],
) -> Result<(String, crate::BuildStage)> {
    if let Ok(distro_id) = parse_distro_id(arg, known_distros) {
        return Ok((distro_id, parse_stage(None)?));
    }

    let stage = parse_stage(Some(arg))?;
    Ok((crate::DEFAULT_DISTRO_ID.to_string(), stage))
}

pub(crate) fn parse_build_two_args(
    arg1: &str,
    arg2: &str,
    known_distros: &[String],
) -> Result<(String, crate::BuildStage)> {
    if let Ok(distro_id) = parse_distro_id(arg1, known_distros) {
        if let Ok(stage) = parse_stage(Some(arg2)) {
            return Ok((distro_id, stage));
        }
    }

    if let Ok(stage) = parse_stage(Some(arg1)) {
        let distro_id = parse_distro_id(arg2, known_distros)?;
        return Ok((distro_id, stage));
    }

    let known_distros = known_distros.join(", ");
    bail!(
        "invalid `iso build` arguments: '{}' '{}'. Expected `<distro> <stage>` or `<stage> <distro>`.\n\
         `stage` supports aliases: 0, 00, 01, 1, 02, 2.\n\
         available distros: {}",
        arg1,
        arg2,
        known_distros
    )
}

pub(crate) fn parse_distro_id(value: &str, known_distros: &[String]) -> Result<String> {
    if let Some(distro_id) = known_distros.iter().find(|d| d.as_str() == value) {
        return Ok(distro_id.to_string());
    }

    let supported = known_distros.join(", ");
    bail!(
        "unsupported distro '{}'; expected one of: {}",
        value,
        supported
    )
}

pub(crate) fn parse_stage(value: Option<&str>) -> Result<crate::BuildStage> {
    match value.unwrap_or(crate::STAGE00_CANONICAL) {
        crate::STAGE00_CANONICAL | "0" | "00" => Ok(crate::BuildStage {
            canonical: crate::STAGE00_CANONICAL,
            slug: crate::STAGE00_SLUG,
            dir_name: crate::STAGE00_DIRNAME,
        }),
        crate::STAGE01_CANONICAL | "1" | "01" => Ok(crate::BuildStage {
            canonical: crate::STAGE01_CANONICAL,
            slug: crate::STAGE01_SLUG,
            dir_name: crate::STAGE01_DIRNAME,
        }),
        crate::STAGE02_CANONICAL | "2" | "02" => Ok(crate::BuildStage {
            canonical: crate::STAGE02_CANONICAL,
            slug: crate::STAGE02_SLUG,
            dir_name: crate::STAGE02_DIRNAME,
        }),
        other => bail!(
            "unsupported stage '{}'; expected one of: '{}', '{}', '{}'; aliases: 0|00|01|1, 02|2",
            other,
            crate::STAGE00_CANONICAL,
            crate::STAGE01_CANONICAL,
            crate::STAGE02_CANONICAL
        ),
    }
}

pub(crate) fn discover_distro_ids(repo_root: &Path) -> Result<Vec<String>> {
    let variants_dir = repo_root.join("distro-variants");
    let entries = fs::read_dir(&variants_dir)
        .with_context(|| format!("reading variants directory '{}'", variants_dir.display()))?;

    let mut distro_ids = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "reading entry under variants directory '{}'",
                variants_dir.display()
            )
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if !path.join("00Build.toml").is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|part| part.to_str()) else {
            continue;
        };
        distro_ids.push(name.to_string());
    }

    if distro_ids.is_empty() {
        bail!(
            "no distro variants discovered under '{}'; expected directories with 00Build.toml",
            variants_dir.display()
        );
    }

    distro_ids.sort();
    Ok(distro_ids)
}
