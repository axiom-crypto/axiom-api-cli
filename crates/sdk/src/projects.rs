use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::{
    AxiomSdk, authenticated_get, authenticated_post, authenticated_put, send_request,
    send_request_json,
};

pub trait ProjectSdk {
    fn list_projects(
        &self,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProjectListResponse>;
    fn create_project(&self, name: &str) -> Result<ProjectCreateResponse>;
    fn get_project(&self, project_id: &str) -> Result<ProjectResponse>;
    fn list_project_programs(
        &self,
        project_id: &str,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProgramListResponse>;
    fn move_program_to_project(&self, program_id: &str, project_id: &str) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResponse {
    pub id: String,
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
    pub project_id: String,
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
    pub project_id: String,
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

        let request = authenticated_get(&self.config, &url)?;
        send_request_json(request, "Failed to list projects")
    }

    fn create_project(&self, name: &str) -> Result<ProjectCreateResponse> {
        let url = format!("{}/projects", self.config.api_url);

        let request = authenticated_post(&self.config, &url)?
            .header("Content-Type", "application/json")
            .json(&name);
        send_request_json(request, "Failed to create project")
    }

    fn get_project(&self, project_id: &str) -> Result<ProjectResponse> {
        let url = format!("{}/projects/{}", self.config.api_url, project_id);

        let request = authenticated_get(&self.config, &url)?;
        send_request_json(request, "Failed to get project")
    }

    fn list_project_programs(
        &self,
        project_id: &str,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProgramListResponse> {
        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(20);
        let url = format!(
            "{}/programs?project_id={}&page={}&page_size={}",
            self.config.api_url, project_id, page, page_size
        );

        let request = authenticated_get(&self.config, &url)?;
        send_request_json(request, "Failed to list project programs")
    }

    fn move_program_to_project(&self, program_id: &str, project_id: &str) -> Result<()> {
        let url = format!("{}/programs/{}", self.config.api_url, program_id);
        let request_body = MoveProgramRequest {
            project_id: project_id.to_string(),
        };

        let request = authenticated_put(&self.config, &url)?
            .header("Content-Type", "application/json")
            .json(&request_body);
        send_request(request, "Failed to move program to project")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AxiomConfig, default_console_base_url};

    #[test]
    fn test_project_response_serialization() {
        let project = ProjectResponse {
            id: "123e4567-e89b-12d3-a456-426614174000".to_string(),
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
        let request = MoveProgramRequest {
            project_id: "456".to_string(),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"project_id\":\"456\""));
    }

    #[test]
    fn test_api_key_missing_error() {
        let config = AxiomConfig {
            api_url: "https://api.test.com/v1".to_string(),
            api_key: None, // No API key
            config_id: None,
            console_base_url: Some(default_console_base_url()),
        };
        let sdk = AxiomSdk::new(config);

        let result = sdk.list_projects(None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key not set"));
    }
}
