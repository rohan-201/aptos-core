// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use anyhow::Result;
use aptos_infallible::RwLock;
use aptos_sdk::{
    move_types::account_address::AccountAddress,
    transaction_builder::TransactionFactory,
    types::{transaction::SignedTransaction, LocalAccount},
};
use async_trait::async_trait;
use std::sync::{atomic::AtomicUsize, Arc};

pub mod account_generator;
pub mod accounts_pool_wrapper;
pub mod call_custom_modules;
pub mod nft_mint_and_transfer;
pub mod p2p_transaction_generator;
pub mod publish_modules;
mod publishing;
pub mod transaction_mix_generator;
use self::{
    account_generator::AccountGeneratorCreator, call_custom_modules::CallCustomModulesCreator,
    nft_mint_and_transfer::NFTMintAndTransferGeneratorCreator,
    p2p_transaction_generator::P2PTransactionGeneratorCreator,
    publish_modules::PublishPackageCreator,
    transaction_mix_generator::PhasedTxnMixGeneratorCreator,
};
use crate::accounts_pool_wrapper::AccountsPoolWrapperCreator;
pub use publishing::module_simple::EntryPoints;

pub const SEND_AMOUNT: u64 = 1;

#[derive(Debug, Copy, Clone)]
pub enum TransactionType {
    CoinTransfer {
        invalid_transaction_ratio: usize,
        sender_use_account_pool: bool,
    },
    AccountGeneration {
        add_created_accounts_to_pool: bool,
        max_account_working_set: usize,
        creation_balance: u64,
    },
    NftMintAndTransfer,
    PublishPackage {
        use_account_pool: bool,
    },
    CallCustomModules {
        entry_point: EntryPoints,
        num_modules: usize,
        use_account_pool: bool,
    },
}

impl TransactionType {
    pub fn default_coin_transfer() -> Self {
        Self::CoinTransfer {
            invalid_transaction_ratio: 0,
            sender_use_account_pool: false,
        }
    }

    pub fn default_account_generation() -> Self {
        Self::AccountGeneration {
            add_created_accounts_to_pool: true,
            max_account_working_set: 1_000_000,
            creation_balance: 0,
        }
    }

    pub fn default_call_custom_module() -> Self {
        Self::CallCustomModules {
            entry_point: EntryPoints::Nop,
            num_modules: 1,
            use_account_pool: false,
        }
    }

    pub fn default_call_different_modules() -> Self {
        Self::CallCustomModules {
            entry_point: EntryPoints::Nop,
            num_modules: 100,
            use_account_pool: false,
        }
    }
}

impl Default for TransactionType {
    fn default() -> Self {
        Self::default_coin_transfer()
    }
}

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
    async fn get_account_balance(&self, account_address: AccountAddress) -> Result<u64>;

    async fn query_sequence_number(&self, account_address: AccountAddress) -> Result<u64>;

    async fn execute_transactions(&self, txns: &[SignedTransaction]) -> Result<()> {
        self.execute_transactions_with_counter(txns, &[AtomicUsize::new(0)])
            .await
    }

    async fn execute_transactions_with_counter(
        &self,
        txns: &[SignedTransaction],
        failure_counter: &[AtomicUsize],
    ) -> Result<()>;
}

pub async fn create_txn_generator_creator(
    transaction_mix_per_phase: &[Vec<(TransactionType, usize)>],
    num_workers: usize,
    source_accounts: &mut [LocalAccount],
    txn_executor: &dyn TransactionExecutor,
    txn_factory: &TransactionFactory,
    init_txn_factory: &TransactionFactory,
    cur_phase: Arc<AtomicUsize>,
) -> (
    Box<dyn TransactionGeneratorCreator>,
    Arc<RwLock<Vec<AccountAddress>>>,
    Arc<RwLock<Vec<LocalAccount>>>,
) {
    let addresses_pool = Arc::new(RwLock::new(
        source_accounts
            .iter()
            .map(|d| d.address())
            .collect::<Vec<_>>(),
    ));
    let accounts_pool = Arc::new(RwLock::new(Vec::new()));

    let mut txn_generator_creator_mix_per_phase: Vec<
        Vec<(Box<dyn TransactionGeneratorCreator>, usize)>,
    > = Vec::new();

    fn wrap_accounts_pool(
        inner: Box<dyn TransactionGeneratorCreator>,
        use_account_pool: bool,
        accounts_pool: Arc<RwLock<Vec<LocalAccount>>>,
    ) -> Box<dyn TransactionGeneratorCreator> {
        if use_account_pool {
            Box::new(AccountsPoolWrapperCreator::new(inner, accounts_pool))
        } else {
            inner
        }
    }

    for transaction_mix in transaction_mix_per_phase {
        let mut txn_generator_creator_mix: Vec<(Box<dyn TransactionGeneratorCreator>, usize)> =
            Vec::new();
        for (transaction_type, weight) in transaction_mix {
            let txn_generator_creator: Box<dyn TransactionGeneratorCreator> = match transaction_type
            {
                TransactionType::CoinTransfer {
                    invalid_transaction_ratio,
                    sender_use_account_pool,
                } => wrap_accounts_pool(
                    Box::new(P2PTransactionGeneratorCreator::new(
                        txn_factory.clone(),
                        SEND_AMOUNT,
                        addresses_pool.clone(),
                        *invalid_transaction_ratio,
                    )),
                    *sender_use_account_pool,
                    accounts_pool.clone(),
                ),
                TransactionType::AccountGeneration {
                    add_created_accounts_to_pool,
                    max_account_working_set,
                    creation_balance,
                } => Box::new(AccountGeneratorCreator::new(
                    txn_factory.clone(),
                    addresses_pool.clone(),
                    accounts_pool.clone(),
                    *add_created_accounts_to_pool,
                    *max_account_working_set,
                    *creation_balance,
                )),
                TransactionType::NftMintAndTransfer => Box::new(
                    NFTMintAndTransferGeneratorCreator::new(
                        txn_factory.clone(),
                        init_txn_factory.clone(),
                        source_accounts.get_mut(0).unwrap(),
                        txn_executor,
                        num_workers,
                    )
                    .await,
                ),
                TransactionType::PublishPackage { use_account_pool } => wrap_accounts_pool(
                    Box::new(PublishPackageCreator::new(txn_factory.clone())),
                    *use_account_pool,
                    accounts_pool.clone(),
                ),
                TransactionType::CallCustomModules {
                    entry_point,
                    num_modules,
                    use_account_pool,
                } => wrap_accounts_pool(
                    Box::new(
                        CallCustomModulesCreator::new(
                            txn_factory.clone(),
                            init_txn_factory.clone(),
                            source_accounts,
                            txn_executor,
                            *entry_point,
                            *num_modules,
                        )
                        .await,
                    ),
                    *use_account_pool,
                    accounts_pool.clone(),
                ),
            };
            txn_generator_creator_mix.push((txn_generator_creator, *weight));
        }
        txn_generator_creator_mix_per_phase.push(txn_generator_creator_mix)
    }

    (
        Box::new(PhasedTxnMixGeneratorCreator::new(
            txn_generator_creator_mix_per_phase,
            cur_phase,
        )),
        addresses_pool,
        accounts_pool,
    )
}
