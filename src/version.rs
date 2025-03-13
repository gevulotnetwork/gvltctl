//! Version calculation utilities.

use serde::Deserialize;
use shadow_rs::cargo_metadata::{self, Metadata};

shadow_rs::shadow!(build_info);

/// Get gevulot-rs dependency version from cargo metadata.
fn get_gevulot_rs_version(metadata: &Metadata) -> Option<String> {
    const GEVULOT_RS_NAME: &str = "gevulot-rs";
    let gvltctl = metadata.root_package()?;
    let gevulot_rs_version_req = &gvltctl
        .dependencies
        .iter()
        .find(|dep| dep.name == GEVULOT_RS_NAME)?
        .req;

    let gevulot_rs_package = metadata
        .packages
        .iter()
        .filter(|package| package.name == GEVULOT_RS_NAME)
        .find(|package| gevulot_rs_version_req.matches(&package.version))?;

    let version = if gevulot_rs_package
        .source
        .as_ref()
        .is_some_and(cargo_metadata::Source::is_crates_io)
    {
        gevulot_rs_package.version.to_string()
    } else {
        format!(
            "{} ({})",
            &gevulot_rs_package.version, &gevulot_rs_package.id.repr
        )
    };

    Some(version)
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
