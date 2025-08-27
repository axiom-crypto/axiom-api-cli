use axiom_sdk::{AxiomSdk, projects::ProjectSdk};
use clap::{Args, Subcommand};
use comfy_table::Table;
use eyre::Result;

#[derive(Args, Debug)]
pub struct ProjectsCmd {
    #[command(subcommand)]
    command: ProjectsSubcommand,
}

#[derive(Debug, Subcommand)]
enum ProjectsSubcommand {
    /// List all projects
    List {
        /// Page number (default: 1)
        #[arg(long, default_value = "1")]
        page: u32,
        /// Page size (default: 20)
        #[arg(long, default_value = "20")]
        page_size: u32,
    },
    /// Create a new project
    Create {
        /// Name of the project to create
        name: String,
    },
    /// Show details for a specific project
    Show {
        /// Project ID to show details for
        #[arg(long, value_name = "ID")]
        project_id: String,
    },
    /// List programs in a project
    Programs {
        /// Project ID to list programs for
        #[arg(long, value_name = "ID")]
        project_id: String,
        /// Page number (default: 1)
        #[arg(long, default_value = "1")]
        page: u32,
        /// Page size (default: 20)
        #[arg(long, default_value = "20")]
        page_size: u32,
    },
    /// Move a program to a different project
    Move {
        /// Program ID to move
        #[arg(long, value_name = "ID")]
        program_id: String,
        /// Target project ID to move program to
        #[arg(long, value_name = "ID")]
        to_project: String,
    },
}

impl ProjectsCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config);

        match self.command {
            ProjectsSubcommand::List { page, page_size } => {
                let response = sdk.list_projects(Some(page), Some(page_size))?;

                if response.items.is_empty() {
                    println!("No projects found");
                    return Ok(());
                }

                let mut table = Table::new();
                table.set_header([
                    "ID",
                    "Name",
                    "Programs",
                    "Total Proofs",
                    "Created By",
                    "Last Active",
                ]);

                for project in response.items {
                    let last_active = project.last_active_at.as_deref().unwrap_or("-").to_string();
                    table.add_row([
                        project.id,
                        project.name,
                        project.program_count.to_string(),
                        project.total_proofs_run.to_string(),
                        project.created_by,
                        last_active,
                    ]);
                }

                println!("{table}");

                let pagination = &response.pagination;
                println!(
                    "Showing page {} of {} (total: {} projects)",
                    pagination.page, pagination.pages, pagination.total
                );

                Ok(())
            }
            ProjectsSubcommand::Create { name } => {
                let response = sdk.create_project(&name)?;

                // Save this as the current project
                axiom_sdk::set_project_id(&response.id)?;

                println!("✓ Created project '{}' with ID: {}", name, response.id);
                println!("✓ Saved project ID {} for future use", response.id);
                Ok(())
            }
            ProjectsSubcommand::Show { project_id } => {
                let project = sdk.get_project(&project_id)?;

                println!("Project Details:");
                println!("  ID: {}", project.id);
                println!("  Name: {}", project.name);
                println!("  Program Count: {}", project.program_count);
                println!("  Total Proofs Run: {}", project.total_proofs_run);
                println!("  Created By: {}", project.created_by);
                println!("  Created At: {}", project.created_at);

                if let Some(last_active) = project.last_active_at {
                    println!("  Last Active At: {}", last_active);
                } else {
                    println!("  Last Active At: -");
                }

                Ok(())
            }
            ProjectsSubcommand::Programs {
                project_id,
                page,
                page_size,
            } => {
                let response =
                    sdk.list_project_programs(&project_id, Some(page), Some(page_size))?;

                if response.items.is_empty() {
                    println!("No programs found in project {}", project_id);
                    return Ok(());
                }

                let mut table = Table::new();
                table.set_header(["Program ID", "Name", "Created At"]);

                for program in response.items {
                    let name = program.name.unwrap_or_else(|| "-".to_string());
                    table.add_row([program.id, name, program.created_at]);
                }

                println!("{table}");

                let pagination = &response.pagination;
                println!(
                    "Showing page {} of {} (total: {} programs)",
                    pagination.page, pagination.pages, pagination.total
                );

                Ok(())
            }
            ProjectsSubcommand::Move {
                program_id,
                to_project,
            } => {
                sdk.move_program_to_project(&program_id, &to_project)?;
                println!(
                    "✓ Successfully moved program {} to project {}",
                    program_id, to_project
                );
                Ok(())
            }
        }
    }
}
