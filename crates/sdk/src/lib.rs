use config::AxiomConfig;

pub mod build;
pub mod config;
pub mod prove;
pub mod verify;
pub mod vm_config;

#[derive(Default)]
pub struct AxiomSdk {
    pub config: AxiomConfig,
}

impl AxiomSdk {
    pub fn new(config: AxiomConfig) -> Self {
        Self { config }
    }
}
