use eyre::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::{AxiomSdk, API_KEY_HEADER};

pub trait ProjectSdk {
    fn list_projects(
        &self,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProjectListResponse>;
    fn create_project(&self, name: &str) -> Result<ProjectCreateResponse>;
    fn get_project(&self, project_id: u32) -> Result<ProjectResponse>;
    fn list_project_programs(
        &self,
        project_id: u32,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProgramListResponse>;
    fn move_program_to_project(&self, program_id: &str, project_id: u32) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResponse {
    pub id: u32,
    pub name: String,
    pub created_at: String,
    pub created_by: String,
    pub program_count: u32,
    pub total_proofs_run: u32,
    pub last_active_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectListResponse {
    pub items: Vec<ProjectResponse>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectCreateResponse {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramResponse {
    pub id: String,
    pub name: Option<String>,
    pub project_id: u32,
    pub project_name: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProgramListResponse {
    pub items: Vec<ProgramResponse>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginationInfo {
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
    pub pages: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct MoveProgramRequest {
    pub project_id: u32,
}

impl ProjectSdk for AxiomSdk {
    fn list_projects(
        &self,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProjectListResponse> {
        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(20);
        let url = format!(
            "{}/projects?page={}&page_size={}",
            self.config.api_url, page, page_size
        );

        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .get(&url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .context("Failed to send list projects request")?;

        if response.status().is_success() {
            let projects: ProjectListResponse = response.json()?;
            Ok(projects)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "List projects request failed with status: {}",
                response.status()
            ))
        }
    }

    fn create_project(&self, name: &str) -> Result<ProjectCreateResponse> {
        let url = format!("{}/projects", self.config.api_url);

        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .post(&url)
            .header(API_KEY_HEADER, api_key)
            .header("Content-Type", "application/json")
            .json(&name)
            .send()
            .context("Failed to send create project request")?;

        if response.status().is_success() {
            let result: ProjectCreateResponse = response.json()?;
            Ok(result)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "Create project request failed with status: {}",
                response.status()
            ))
        }
    }

    fn get_project(&self, project_id: u32) -> Result<ProjectResponse> {
        let url = format!("{}/projects/{}", self.config.api_url, project_id);

        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .get(&url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .context("Failed to send get project request")?;

        if response.status().is_success() {
            let project: ProjectResponse = response.json()?;
            Ok(project)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "Get project request failed with status: {}",
                response.status()
            ))
        }
    }

    fn list_project_programs(
        &self,
        project_id: u32,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProgramListResponse> {
        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(20);
        let url = format!(
            "{}/programs?project_id={}&page={}&page_size={}",
            self.config.api_url, project_id, page, page_size
        );

        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .get(&url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .context("Failed to send list project programs request")?;

        if response.status().is_success() {
            let programs: ProgramListResponse = response.json()?;
            Ok(programs)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "List project programs request failed with status: {}",
                response.status()
            ))
        }
    }

    fn move_program_to_project(&self, program_id: &str, project_id: u32) -> Result<()> {
        let url = format!("{}/programs/{}", self.config.api_url, program_id);

        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let request_body = MoveProgramRequest { project_id };

        let response = client
            .put(&url)
            .header(API_KEY_HEADER, api_key)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .context("Failed to send move program request")?;

        if response.status().is_success() {
            Ok(())
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "Move program request failed with status: {}",
                response.status()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxiomConfig;

    #[test]
    fn test_project_response_serialization() {
        let project = ProjectResponse {
            id: 123,
            name: "Test Project".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            created_by: "test@example.com".to_string(),
            program_count: 5,
            total_proofs_run: 42,
            last_active_at: Some("2025-01-15T10:30:00Z".to_string()),
        };

        let json = serde_json::to_string(&project).unwrap();
        let deserialized: ProjectResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(project.id, deserialized.id);
        assert_eq!(project.name, deserialized.name);
        assert_eq!(project.program_count, deserialized.program_count);
    }

    #[test]
    fn test_move_program_request_serialization() {
        let request = MoveProgramRequest { project_id: 456 };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"project_id\":456"));
    }

    #[test]
    fn test_api_key_missing_error() {
        let config = AxiomConfig {
            api_url: "https://api.test.com/v1".to_string(),
            api_key: None, // No API key
            config_id: None,
            last_project_id: None,
        };
        let sdk = AxiomSdk::new(config);

        let result = sdk.list_projects(None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key not set"));
    }
}
