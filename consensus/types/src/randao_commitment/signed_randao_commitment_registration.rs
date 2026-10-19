use bls::SignatureBytes;
use serde::{Deserialize, Serialize};
use ssz_derive::{Decode, Encode};
use tree_hash_derive::TreeHash;

use super::RandaoCommitmentRegistration;

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Debug, PartialEq, Hash, Clone, Serialize, Deserialize, Encode, Decode, TreeHash)]
pub struct SignedRandaoCommitmentRegistration {
    pub message: RandaoCommitmentRegistration,
    pub signature: SignatureBytes, // BLSSignature in the consensus spec
}

#[cfg(test)]
mod tests {
    use super::*;

    ssz_and_tree_hash_tests!(SignedRandaoCommitmentRegistration);
}
