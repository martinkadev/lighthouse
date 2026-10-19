use serde::{Deserialize, Serialize};
use ssz_derive::{Decode, Encode};
use tree_hash_derive::TreeHash;

use crate::core::{Epoch, Hash256};

type ValidatorIndex = u64;

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize, Encode, Decode, TreeHash)]
pub struct PendingRandaoCommitment {
    #[serde(with = "serde_utils::quoted_u64")]
    pub validator_index: ValidatorIndex,
    pub commitment: Hash256, // because B256<32> in aliases.rs
    pub activation_epoch: Epoch,
}

#[cfg(test)]
mod tests {
    use super::*;

    ssz_and_tree_hash_tests!(PendingRandaoCommitment);
}
