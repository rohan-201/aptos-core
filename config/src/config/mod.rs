// Copyright © Aptos Foundation
// Parts of the project are originally copyright © Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::network_id::NetworkId;
use aptos_crypto::x25519;
use aptos_types::PeerId;
use rand::{rngs::StdRng, SeedableRng};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

// All modules should be declared below
mod api_config;
mod base_config;
mod consensus_config;
mod error;
mod execution_config;
mod identity_config;
mod indexer_config;
mod indexer_grpc_config;
mod inspection_service_config;
mod logger_config;
mod mempool_config;
mod network_config;
mod quorum_store_config;
mod safety_rules_config;
mod secure_backend_config;
mod state_sync_config;
mod storage_config;
mod test_config;

// All public usage statements should be declared below
pub use api_config::*;
pub use base_config::*;
pub use consensus_config::*;
pub use error::*;
pub use execution_config::*;
pub use identity_config::*;
pub use indexer_config::*;
pub use indexer_grpc_config::*;
pub use inspection_service_config::*;
pub use logger_config::*;
pub use mempool_config::*;
pub use network_config::*;
pub use quorum_store_config::*;
pub use safety_rules_config::*;
pub use secure_backend_config::*;
pub use state_sync_config::*;
pub use storage_config::*;
pub use test_config::*;

/// The node configuration defines the configuration for a single Aptos
/// node (i.e., validator or fullnode). It is composed of module
/// configurations for each of the modules that the node uses (e.g.,
/// the API, indexer, mempool, state sync, etc.).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub base: BaseConfig,
    #[serde(default)]
    pub consensus: ConsensusConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub failpoints: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub full_node_networks: Vec<NetworkConfig>,
    #[serde(default)]
    pub indexer: IndexerConfig,
    #[serde(default)]
    pub indexer_grpc: IndexerGrpcConfig,
    #[serde(default)]
    pub inspection_service: InspectionServiceConfig,
    #[serde(default)]
    pub logger: LoggerConfig,
    #[serde(default)]
    pub mempool: MempoolConfig,
    #[serde(default)]
    pub peer_monitoring_service: PeerMonitoringServiceConfig,
    #[serde(default)]
    pub state_sync: StateSyncConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub test: Option<TestConfig>,
    #[serde(default)]
    pub validator_network: Option<NetworkConfig>,
}

impl NodeConfig {
    pub fn data_dir(&self) -> &Path {
        &self.base.data_dir
    }

    pub fn working_dir(&self) -> &Path {
        match &self.base.working_dir {
            Some(working_dir) => working_dir,
            None => &self.base.data_dir,
        }
    }

    pub fn set_data_dir(&mut self, data_dir: PathBuf) {
        self.base.data_dir = data_dir.clone();
        self.consensus.set_data_dir(data_dir.clone());
        self.storage.set_data_dir(data_dir);
    }

    /// Reads the config file and returns the configuration object in addition to doing some
    /// post-processing of the config.
    /// Paths used in the config are either absolute or relative to the config location.
    pub fn load<P: AsRef<Path>>(input_path: P) -> Result<Self, Error> {
        let mut config = Self::load_config(&input_path)?;

        let input_dir = RootPath::new(input_path);
        config.execution.load(&input_dir)?;

        let mut config = config
            .validate_indexer_configs()?
            .validate_indexer_grpc_configs()?
            .validate_network_configs()?;
        config.set_data_dir(config.data_dir().to_path_buf());
        Ok(config)
    }

    pub fn peer_id(&self) -> Option<PeerId> {
        match self.base.role {
            RoleType::Validator => self.validator_network.as_ref().map(NetworkConfig::peer_id),
            RoleType::FullNode => self
                .full_node_networks
                .iter()
                .find(|config| config.network_id == NetworkId::Public)
                .map(NetworkConfig::peer_id),
        }
    }

    pub fn identity_key(&self) -> Option<x25519::PrivateKey> {
        match self.base.role {
            RoleType::Validator => self
                .validator_network
                .as_ref()
                .map(NetworkConfig::identity_key),
            RoleType::FullNode => self
                .full_node_networks
                .iter()
                .find(|config| config.network_id == NetworkId::Public)
                .map(NetworkConfig::identity_key),
        }
    }

    /// Validate `IndexerConfig`, ensuring that it's set up correctly
    /// Additionally, handles any strange missing default cases
    fn validate_indexer_configs(mut self) -> Result<NodeConfig, Error> {
        if !self.indexer.enabled {
            return Ok(self);
        }

        self.indexer.postgres_uri = env_or_default(
            "INDEXER_DATABASE_URL",
            self.indexer.postgres_uri,
            must_be_set("postgres_uri", "INDEXER_DATABASE_URL"),
        );

        self.indexer.processor = env_or_default(
            "PROCESSOR_NAME",
            self.indexer
                .processor
                .or_else(|| Some("default_processor".to_string())),
            None,
        );

        self.indexer.starting_version = match std::env::var("STARTING_VERSION").ok() {
            None => self.indexer.starting_version,
            Some(s) => match s.parse::<u64>() {
                Ok(version) => Some(version),
                Err(_) => {
                    // Doing this instead of failing. This will allow a processor to have STARTING_VERSION: undefined when deploying
                    aptos_logger::warn!(
                        "Invalid STARTING_VERSION: {}, using {:?} instead",
                        s,
                        self.indexer.starting_version
                    );
                    self.indexer.starting_version
                },
            },
        };

        self.indexer.skip_migrations = self.indexer.skip_migrations.or(Some(false));
        self.indexer.check_chain_id = self.indexer.check_chain_id.or(Some(true));
        self.indexer.batch_size = default_if_zero(
            self.indexer.batch_size.map(|v| v as u64),
            DEFAULT_BATCH_SIZE as u64,
        )
        .map(|v| v as u16);
        self.indexer.fetch_tasks = default_if_zero(
            self.indexer.fetch_tasks.map(|v| v as u64),
            DEFAULT_FETCH_TASKS as u64,
        )
        .map(|v| v as u8);
        self.indexer.processor_tasks =
            default_if_zero_u8(self.indexer.processor_tasks, DEFAULT_PROCESSOR_TASKS);
        self.indexer.emit_every = self.indexer.emit_every.or(Some(0));
        self.indexer.gap_lookback_versions = env_or_default(
            "GAP_LOOKBACK_VERSIONS",
            self.indexer.gap_lookback_versions.or(Some(1_500_000)),
            None,
        );

        Ok(self)
    }

    /// Validate `IndexerGrpcConfig`, ensuring that it's set up correctly
    /// Additionally, handles any strange missing default cases
    fn validate_indexer_grpc_configs(mut self) -> Result<NodeConfig, Error> {
        if !self.indexer_grpc.enabled {
            return Ok(self);
        }

        self.indexer_grpc.address = self
            .indexer_grpc
            .address
            .or_else(|| Some("0.0.0.0:50051".to_string()));

        self.indexer_grpc.processor_task_count =
            self.indexer_grpc.processor_task_count.or(Some(20));

        self.indexer_grpc.processor_batch_size =
            self.indexer_grpc.processor_batch_size.or(Some(1000));

        self.indexer_grpc.output_batch_size = self.indexer_grpc.output_batch_size.or(Some(100));

        Ok(self)
    }

    /// Checks `NetworkConfig` setups so that they exist on proper networks
    /// Additionally, handles any strange missing default cases
    fn validate_network_configs(mut self) -> Result<NodeConfig, Error> {
        if self.base.role.is_validator() {
            invariant(
                self.validator_network.is_some(),
                "Missing a validator network config for a validator node".into(),
            )?;
        } else {
            invariant(
                self.validator_network.is_none(),
                "Provided a validator network config for a full_node node".into(),
            )?;
        }

        if let Some(network) = &mut self.validator_network {
            network.load_validator_network()?;
            network.mutual_authentication = true; // This should always be the default for validators
        }
        for network in &mut self.full_node_networks {
            network.load_fullnode_network()?;
        }
        Ok(self)
    }

    pub fn save<P: AsRef<Path>>(&mut self, output_path: P) -> Result<(), Error> {
        let output_dir = RootPath::new(&output_path);
        self.execution.save(&output_dir)?;
        // This must be last as calling save on subconfigs may change their fields
        self.save_config(&output_path)?;
        Ok(())
    }

    pub fn randomize_ports(&mut self) {
        self.api.randomize_ports();
        self.inspection_service.randomize_ports();
        self.storage.randomize_ports();
        self.logger.disable_console();

        if let Some(network) = self.validator_network.as_mut() {
            network.listen_address = crate::utils::get_available_port_in_multiaddr(true);
        }

        for network in self.full_node_networks.iter_mut() {
            network.listen_address = crate::utils::get_available_port_in_multiaddr(true);
        }
    }

    pub fn random() -> Self {
        let mut rng = StdRng::from_seed([0u8; 32]);
        Self::random_with_template(0, &NodeConfig::default(), &mut rng)
    }

    pub fn random_with_template(_idx: u32, template: &Self, rng: &mut StdRng) -> Self {
        let mut config = template.clone();
        config.random_internal(rng);
        config
    }

    fn random_internal(&mut self, rng: &mut StdRng) {
        let mut test = TestConfig::new_with_temp_dir(None);

        if self.base.role == RoleType::Validator {
            test.random_account_key(rng);
            let peer_id = test.auth_key.unwrap().derived_address();

            if self.validator_network.is_none() {
                let network_config = NetworkConfig::network_with_id(NetworkId::Validator);
                self.validator_network = Some(network_config);
            }

            let validator_network = self.validator_network.as_mut().unwrap();
            validator_network.random_with_peer_id(rng, Some(peer_id));
            // We want to produce this key twice
            test.random_execution_key(rng);

            let mut safety_rules_test_config = SafetyRulesTestConfig::new(peer_id);
            safety_rules_test_config.random_consensus_key(rng);
            self.consensus.safety_rules.test = Some(safety_rules_test_config);
        } else {
            self.validator_network = None;
            if self.full_node_networks.is_empty() {
                let network_config = NetworkConfig::network_with_id(NetworkId::Public);
                self.full_node_networks.push(network_config);
            }
            for network in &mut self.full_node_networks {
                network.random(rng);
            }
        }
        self.set_data_dir(test.temp_dir().unwrap().to_path_buf());
        self.test = Some(test);
    }

    fn default_config(serialized: &str, path: &'static str) -> Self {
        let config = Self::parse(serialized).unwrap_or_else(|e| panic!("Error in {}: {}", path, e));
        config
            .validate_indexer_configs()
            .unwrap_or_else(|e| panic!("Error in {}: {}", path, e))
            .validate_network_configs()
            .unwrap_or_else(|e| panic!("Error in {}: {}", path, e))
    }

    pub fn default_for_public_full_node() -> Self {
        let contents = std::include_str!("test_data/public_full_node.yaml");
        Self::default_config(contents, "default_for_public_full_node")
    }

    pub fn default_for_validator() -> Self {
        let contents = std::include_str!("test_data/validator.yaml");
        Self::default_config(contents, "default_for_validator")
    }

    pub fn default_for_validator_full_node() -> Self {
        let contents = std::include_str!("test_data/validator_full_node.yaml");
        Self::default_config(contents, "default_for_validator_full_node")
    }
}

pub trait PersistableConfig: Serialize + DeserializeOwned {
    fn load_config<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        // Open the file and read it into a string
        let config_path_string = path.as_ref().to_str().unwrap().to_string();
        let mut file = File::open(&path).map_err(|error| {
            Error::Unexpected(format!(
                "Failed to open config file: {:?}. Error: {:?}",
                config_path_string, error
            ))
        })?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).map_err(|error| {
            Error::Unexpected(format!(
                "Failed to read the config file into a string: {:?}. Error: {:?}",
                config_path_string, error
            ))
        })?;

        // Parse the file string
        Self::parse(&contents)
    }

    fn save_config<P: AsRef<Path>>(&self, output_file: P) -> Result<(), Error> {
        let contents = serde_yaml::to_vec(&self)
            .map_err(|e| Error::Yaml(output_file.as_ref().to_str().unwrap().to_string(), e))?;
        let mut file = File::create(output_file.as_ref())
            .map_err(|e| Error::IO(output_file.as_ref().to_str().unwrap().to_string(), e))?;
        file.write_all(&contents)
            .map_err(|e| Error::IO(output_file.as_ref().to_str().unwrap().to_string(), e))?;
        Ok(())
    }

    fn parse(serialized: &str) -> Result<Self, Error> {
        serde_yaml::from_str(serialized).map_err(|e| Error::Yaml("config".to_string(), e))
    }
}

// We only implement PersistableConfig for the configs that should be read/written to disk
impl PersistableConfig for NodeConfig {}
impl PersistableConfig for SafetyRulesConfig {}

#[derive(Debug)]
pub struct RootPath {
    root_path: PathBuf,
}

impl RootPath {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let root_path = if let Some(parent) = path.as_ref().parent() {
            parent.to_path_buf()
        } else {
            PathBuf::from("")
        };

        Self { root_path }
    }

    /// This function assumes that the path is already a directory
    pub fn new_path<P: AsRef<Path>>(path: P) -> Self {
        let root_path = path.as_ref().to_path_buf();
        Self { root_path }
    }

    /// This adds a full path when loading / storing if one is not specified
    pub fn full_path(&self, file_path: &Path) -> PathBuf {
        if file_path.is_relative() {
            self.root_path.join(file_path)
        } else {
            file_path.to_path_buf()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn verify_configs() {
        NodeConfig::default_for_public_full_node();
        NodeConfig::default_for_validator();
        NodeConfig::default_for_validator_full_node();

        let contents = std::include_str!("test_data/safety_rules.yaml");
        SafetyRulesConfig::parse(contents)
            .unwrap_or_else(|e| panic!("Error in safety_rules.yaml: {}", e));
    }

    #[test]
    fn validate_invalid_network_id() {
        let mut config = NodeConfig::default_for_public_full_node();
        let network = config.full_node_networks.iter_mut().next().unwrap();
        network.network_id = NetworkId::Validator;
        assert!(matches!(
            config.validate_network_configs(),
            Err(Error::InvariantViolation(_))
        ));
    }
}
