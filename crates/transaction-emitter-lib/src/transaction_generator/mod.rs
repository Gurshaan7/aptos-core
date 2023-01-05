// Copyright (c) Aptos
// SPDX-License-Identifier: Apache-2.0

use aptos_sdk::types::{transaction::SignedTransaction, LocalAccount};
use async_trait::async_trait;

pub mod account_generator;
pub mod call_custom_modules;
pub mod nft_mint_and_transfer;
pub mod p2p_transaction_generator;
pub mod publish_modules;
mod publishing;
pub mod transaction_mix_generator;
pub use publishing::module_simple::EntryPoints;

pub trait TransactionGenerator: Sync + Send {
    fn generate_transactions(
        &mut self,
        accounts: Vec<&mut LocalAccount>,
        transactions_per_account: usize,
    ) -> Vec<SignedTransaction>;
}

#[async_trait]
pub trait TransactionGeneratorCreator: Sync + Send {
    async fn create_transaction_generator(&mut self) -> Box<dyn TransactionGenerator>;
}

#[async_trait]
pub trait TransactionExecutor: Sync + Send {
    async fn execute_transactions(&self, txns: &[SignedTransaction]);
}
