use crate::decode::yaml_decode_file;
use crate::{Error, FeatureName, testing_spec};
use serde::Deserialize;
use ssz::Encode;
use std::fmt::Debug;
use std::path::Path;
use types::{ChainSpec, Config, EthSpec, ForkName};

pub(super) fn load_config(path: &Path) -> Result<Option<Config>, Error> {
    let config_path = path.join("config.yaml");
    if config_path.is_file() {
        yaml_decode_file(&config_path).map(Some)
    } else {
        Ok(None)
    }
}

pub(super) fn testing_spec_with_config<E: EthSpec>(
    path: &Path,
    fork_name: ForkName,
    config: Option<&Config>,
) -> Result<ChainSpec, Error> {
    let spec = testing_spec::<E>(fork_name);
    let mut spec = match config {
        Some(config) => config.apply_to_chain_spec::<E>(&spec).ok_or_else(|| {
            Error::FailedToParseTest("config does not match the preset or slot duration".into())
        })?,
        None => spec,
    };

    // Config does not include experimental feature keys. Apply them after the case's
    // ordinary config so HEZE_FORK_VERSION cannot replace EIP8321_FORK_VERSION.
    apply_case_feature_version::<E>(path, fork_name, &mut spec)?;
    Ok(spec)
}

pub(super) fn testing_spec_for_case<E: EthSpec>(
    path: &Path,
    fork_name: ForkName,
) -> Result<ChainSpec, Error> {
    testing_spec_with_config::<E>(path, fork_name, None)
}

#[derive(Default, Deserialize)]
struct FeatureVersions {
    #[serde(rename = "EIP8321_FORK_VERSION", default)]
    eip8321_fork_version: Option<String>,
}

fn parse_feature_version(value: &str) -> Result<[u8; 4], Error> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    let bytes = hex::decode(value).map_err(|error| {
        Error::FailedToParseTest(format!("invalid EIP8321_FORK_VERSION: {error}"))
    })?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        Error::FailedToParseTest(format!(
            "EIP8321_FORK_VERSION must contain 4 bytes, got {}",
            bytes.len()
        ))
    })
}

fn apply_case_feature_version<E: EthSpec>(
    path: &Path,
    fork_name: ForkName,
    spec: &mut ChainSpec,
) -> Result<(), Error> {
    // Handler case paths end in <fork_or_feature>/<runner>/<handler>/<suite>/<case>.
    // Only inspect that component: an unrelated parent directory may also be named eip8321.
    let fixture_name = path.ancestors().nth(4).and_then(Path::file_name);
    let feature = FeatureName::list_all()
        .into_iter()
        .find(|feature| fixture_name.is_some_and(|name| name == feature.to_string().as_str()));
    let Some(feature) = feature else {
        return Ok(());
    };
    if feature.fork_name() != fork_name {
        return Err(Error::FailedToParseTest(format!(
            "feature {feature} requires the {} representation, got {fork_name}",
            feature.fork_name()
        )));
    }

    let config_path = path.join("config.yaml");
    let versions: FeatureVersions = if config_path.is_file() {
        yaml_decode_file(&config_path)?
    } else {
        FeatureVersions::default()
    };
    let version = match feature {
        FeatureName::Eip8321 => versions
            .eip8321_fork_version
            .as_deref()
            .map(parse_feature_version)
            .transpose()?,
    };
    feature.apply_fork_version::<E>(spec, version);
    Ok(())
}

/// Trait for types that can be used in SSZ static tests.
pub trait SszStaticType: Encode + Clone + PartialEq + Debug + Sync {}

impl<T> SszStaticType for T where T: Encode + Clone + PartialEq + Debug + Sync {}

#[macro_export]
macro_rules! impl_bls_load_case {
    ($case_name:ident) => {
        use $crate::decode::yaml_decode_file;
        impl LoadCase for $case_name {
            fn load_from_dir(path: &Path, _fork_name: ForkName) -> Result<Self, Error> {
                yaml_decode_file(&path)
            }
        }
    };

    ($case_name:ident, $sub_path_name:expr) => {
        use $crate::decode::yaml_decode_file;
        impl LoadCase for $case_name {
            fn load_from_dir(path: &Path, _fork_name: ForkName) -> Result<Self, Error> {
                yaml_decode_file(&path.join($sub_path_name))
            }
        }
    };
}

#[cfg(test)]
mod feature_version_tests {
    use super::*;
    use types::{Epoch, MainnetEthSpec, MinimalEthSpec};

    #[test]
    fn feature_version_uses_preset_defaults() {
        assert_eq!(
            FeatureName::Eip8321.fork_version::<MainnetEthSpec>(),
            [0xe8, 0x32, 0x10, 0]
        );
        assert_eq!(
            FeatureName::Eip8321.fork_version::<MinimalEthSpec>(),
            [0xe8, 0x32, 0x10, 1]
        );
    }

    #[test]
    fn feature_version_survives_case_config() {
        let mut case_spec = testing_spec::<MinimalEthSpec>(ForkName::Heze);
        case_spec.genesis_delay = 987;
        let config = Config::from_chain_spec::<MinimalEthSpec>(&case_spec);
        let spec = testing_spec_with_config::<MinimalEthSpec>(
            Path::new(
                "feature_version_unit_fixtures/minimal/eip8321/operations/bid/pyspec_tests/case",
            ),
            ForkName::Heze,
            Some(&config),
        )
        .unwrap();
        assert_eq!(spec.heze_fork_version, [0xe8, 0x32, 0x10, 1]);
        assert_eq!(
            spec.fork_version_for_epoch(Epoch::new(0)),
            [0xe8, 0x32, 0x10, 1]
        );
        assert_eq!(spec.genesis_delay, 987);
    }

    #[test]
    fn ordinary_fork_versions_are_unchanged() {
        let heze = testing_spec_for_case::<MinimalEthSpec>(
            Path::new("feature_version_unit_fixtures/eip8321/minimal/heze/operations/bid/pyspec_tests/case"),
            ForkName::Heze,
        )
        .unwrap();
        assert_eq!(heze.heze_fork_version, [0x08, 0, 0, 1]);
        let gloas = testing_spec_for_case::<MinimalEthSpec>(
            Path::new("feature_version_unit_fixtures/eip8321/minimal/gloas/operations/bid/pyspec_tests/case"),
            ForkName::Gloas,
        )
        .unwrap();
        assert_eq!(gloas.fork_version_for_epoch(Epoch::new(0)), [0x07, 0, 0, 1]);
    }

    #[test]
    fn feature_config_version_is_parsed_and_validated() {
        let config: FeatureVersions = crate::decode::yaml_decode(
            "HEZE_FORK_VERSION: 0x08000001\nEIP8321_FORK_VERSION: 0xe8321001\n",
        )
        .unwrap();
        assert_eq!(
            parse_feature_version(config.eip8321_fork_version.as_deref().unwrap()).unwrap(),
            [0xe8, 0x32, 0x10, 1]
        );
        assert!(parse_feature_version("0xe83210").is_err());
        assert!(parse_feature_version("0xnot_hex").is_err());
    }
}
