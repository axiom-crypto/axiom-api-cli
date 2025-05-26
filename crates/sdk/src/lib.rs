use config::AxiomConfig;

pub mod build;
pub mod config;

#[derive(Default)]
pub struct AxiomSdk {
    pub config: AxiomConfig,
}

impl AxiomSdk {
    pub fn new(config: AxiomConfig) -> Self {
        Self { config }
    }
}
