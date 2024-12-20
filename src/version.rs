//! Version calculation utilities.

use cargo_metadata::Metadata;
use serde::Deserialize;

shadow_rs::shadow!(build_info);

/// Get gevulot-rs dependency version from cargo metadata.
fn get_gevulot_rs_version(metadata: &Metadata) -> Option<String> {
    const GEVULOT_RS_NAME: &str = "gevulot-rs";
    let gvltctl = metadata.root_package()?;
    let gevulot_rs_dep = gvltctl
        .dependencies
        .iter()
        .find(|dep| dep.name == GEVULOT_RS_NAME)?;

    if let Some(path) = gevulot_rs_dep.path.as_ref() {
        Some(format!(
            "{} ({})",
            metadata
                .packages
                .iter()
                .find(|package| {
                    package.name == GEVULOT_RS_NAME && package.id.repr.starts_with("path")
                })?
                .version,
            path.as_str()
        ))
    } else if gevulot_rs_dep
        .source
        .as_ref()
        .is_some_and(|src| src.starts_with("git"))
    {
        metadata.packages.iter().find_map(|package| {
            if package.name == GEVULOT_RS_NAME {
                package
                    .id
                    .repr
                    .strip_prefix("git+")?
                    .split('#')
                    .collect::<Vec<_>>()
                    .first()
                    .map(|id| format!("{} ({})", package.version, id))
            } else {
                None
            }
        })
    } else if gevulot_rs_dep
        .source
        .as_ref()
        .is_some_and(|src| src.starts_with("registry"))
    {
        metadata.packages.iter().find_map(|package| {
            if package.name == GEVULOT_RS_NAME
                && package
                    .source
                    .as_ref()
                    .is_some_and(cargo_metadata::Source::is_crates_io)
            {
                Some(package.version.to_string())
            } else {
                None
            }
        })
    } else {
        return None;
    }
}

/// Get long version of the tool.
///
/// This includes:
/// - package version
/// - git info
/// - gevulot-rs version
/// - platform info
#[allow(clippy::const_is_empty)]
pub fn get_long_version() -> String {
    let gevulot_rs_version =
        serde_json::from_slice::<serde_json::Value>(build_info::CARGO_METADATA)
            .ok()
            .map(Metadata::deserialize)
            .and_then(Result::ok)
            .as_ref()
            .and_then(get_gevulot_rs_version);
    format!(
        "{} ({})\ngevulot-rs {}\nplatform: {}",
        build_info::PKG_VERSION,
        if build_info::GIT_CLEAN {
            format!(
                "{} {}",
                if build_info::TAG.is_empty() {
                    build_info::SHORT_COMMIT
                } else {
                    build_info::TAG
                },
                // Strip commit time and leave only date
                build_info::COMMIT_DATE.split(' ').collect::<Vec<_>>()[0],
            )
        } else {
            format!("{}-dirty", build_info::SHORT_COMMIT)
        },
        gevulot_rs_version.unwrap_or_else(|| "unknown".to_string()),
        build_info::BUILD_TARGET,
    )
}
