use super::common::testing_spec_for_case;
use super::*;
use crate::case_result::compare_beacon_state_results_without_caches;
use crate::decode::{ssz_decode_file, ssz_decode_state, yaml_decode_file};
use serde::Deserialize;
use state_processing::GloasVerificationContext;
use state_processing::upgrade::{
    upgrade_to_altair, upgrade_to_bellatrix, upgrade_to_capella, upgrade_to_deneb,
    upgrade_to_eip8321, upgrade_to_electra, upgrade_to_fulu, upgrade_to_gloas, upgrade_to_heze,
};
use types::BeaconState;

mod pre_eip8321;
use pre_eip8321::PreEip8321State;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Metadata {
    pub fork: String,
}

impl Metadata {
    fn fork_name(&self) -> ForkName {
        self.fork.parse().unwrap()
    }
}

#[derive(Debug)]
pub struct ForkTest<E: EthSpec> {
    path: PathBuf,
    pub metadata: Metadata,
    pub pre: BeaconState<E>,
    pub post: BeaconState<E>,
}

impl<E: EthSpec> LoadCase for ForkTest<E> {
    fn load_from_dir(path: &Path, fork_name: ForkName) -> Result<Self, Error> {
        let metadata: Metadata = yaml_decode_file(&path.join("meta.yaml"))?;
        let is_eip8321 = metadata.fork == "eip8321";

        if is_eip8321 {
            assert_eq!(fork_name, ForkName::Heze);
        } else {
            assert_eq!(metadata.fork_name(), fork_name);
        }

        let pre = if is_eip8321 {
            ssz_decode_file::<PreEip8321State<E>>(&path.join("pre.ssz_snappy"))?
                .into_feature_state()
        } else {
            let pre_fork = fork_name.previous_fork().unwrap_or(ForkName::Base);
            let pre_spec = &pre_fork.make_genesis_spec(E::default_spec());
            ssz_decode_state(&path.join("pre.ssz_snappy"), pre_spec)?
        };

        // Decode post-state with target fork.
        let post_spec = &testing_spec_for_case::<E>(path, fork_name)?;
        let post = ssz_decode_state(&path.join("post.ssz_snappy"), post_spec)?;

        Ok(Self {
            path: path.to_path_buf(),
            metadata,
            pre,
            post,
        })
    }
}

impl<E: EthSpec> Case for ForkTest<E> {
    fn is_enabled_for_fork(fork_name: ForkName) -> bool {
        // Upgrades exist targeting all forks except phase0/base.
        // Fork tests also need BLS.
        cfg!(not(feature = "fake_crypto")) && fork_name != ForkName::Base
    }

    fn is_enabled_for_feature(feature_name: FeatureName) -> bool {
        cfg!(not(feature = "fake_crypto")) && feature_name == FeatureName::Eip8321
    }

    fn result(&self, _case_index: usize, fork_name: ForkName) -> Result<(), Error> {
        let mut result_state = self.pre.clone();
        let mut expected = Some(self.post.clone());
        let spec = &testing_spec_for_case::<E>(&self.path, fork_name)?;

        let mut result = if self.metadata.fork == "eip8321" {
            upgrade_to_eip8321(&mut result_state, spec.heze_fork_version).map(|_| result_state)
        } else {
            match fork_name {
                ForkName::Base => panic!("phase0 not supported"),
                ForkName::Altair => {
                    upgrade_to_altair(&mut result_state, spec).map(|_| result_state)
                }
                ForkName::Bellatrix => {
                    upgrade_to_bellatrix(&mut result_state, spec).map(|_| result_state)
                }
                ForkName::Capella => {
                    upgrade_to_capella(&mut result_state, spec).map(|_| result_state)
                }
                ForkName::Deneb => upgrade_to_deneb(&mut result_state, spec).map(|_| result_state),
                ForkName::Electra => {
                    upgrade_to_electra(&mut result_state, spec).map(|_| result_state)
                }
                ForkName::Fulu => upgrade_to_fulu(&mut result_state, spec).map(|_| result_state),
                ForkName::Gloas => upgrade_to_gloas(
                    &mut result_state,
                    GloasVerificationContext::FullVerification,
                    spec,
                )
                .map(|_| result_state),
                ForkName::Heze => upgrade_to_heze(&mut result_state, spec).map(|_| result_state),
            }
        };

        compare_beacon_state_results_without_caches(&mut result, &mut expected)
    }
}
