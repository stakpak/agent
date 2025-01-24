use std::{path::PathBuf, sync::Arc, time::Duration};
use uuid::Uuid;

use crate::{
    client::{
        models::{
            Action, ActionStatus, AgentID, AgentInput, AgentSessionVisibility, AgentStatus,
            RunAgentInput, RunCommandArgs,
        },
        Client,
    },
    commands::agent::get_next_input,
    config::AppConfig,
    utils::{output::setup_output_handler, socket::SocketClient},
};

pub async fn run_agent(
    config: &AppConfig,
    client: &Client,
    agent_id: AgentID,
    checkpoint_id: Option<String>,
    input: Option<AgentInput>,
    short_circuit_actions: bool,
) -> Result<Uuid, String> {
    let (agent_id, session, checkpoint) = match checkpoint_id {
        Some(checkpoint_id) => {
            let checkpoint_uuid = Uuid::parse_str(&checkpoint_id).map_err(|_| {
                format!(
                    "Invalid checkpoint ID '{}' - must be a valid UUID",
                    checkpoint_id
                )
            })?;

            let output = client.get_agent_checkpoint(checkpoint_uuid).await?;

            (
                output.output.get_agent_id(),
                output.session,
                output.checkpoint,
            )
        }
        None => {
            let session = client
                .create_agent_session(
                    agent_id.clone(),
                    AgentSessionVisibility::Private,
                    input.clone(),
                )
                .await?;

            let checkpoint = session
                .checkpoints
                .first()
                .ok_or("No checkpoint found in new session")?
                .clone();

            (agent_id, session.into(), checkpoint)
        }
    };

    let socket_client = Arc::new(
        SocketClient::connect(config, session.id.to_string())
            .await
            .unwrap(),
    );

    let print = setup_output_handler(socket_client.clone());

    let mut input = RunAgentInput {
        checkpoint_id: checkpoint.id,
        input: match input {
            Some(input) => input,
            None => AgentInput::new(&agent_id),
        },
    };

    loop {
        print("[ ▄▀ Stakpaking... ]");
        let output = client.run_agent(&input).await?;
        print(&format!(
            "[Current Checkpoint {} (Agent Status: {})]",
            output.checkpoint.id, output.checkpoint.status
        ));

        input = get_next_input(&agent_id, client, &print, &output, short_circuit_actions).await?;

        match output.checkpoint.status {
            AgentStatus::Complete => {
                print("[Mission Accomplished]");
                break;
            }
            AgentStatus::Failed => {
                print("[Mission Failed :'(]");
                break;
            }
            _ => {}
        };
    }

    Ok(input.checkpoint_id)
}

pub async fn run_terraform_agent(
    config: &AppConfig,
    client: &Client,
    dir: Option<String>,
) -> Result<Uuid, String> {
    let dir_arg = dir
        .as_ref()
        .map(|d| format!("-chdir={}", d))
        .unwrap_or_default();

    let action_queue = vec![
        Action::RunCommand {
            id: Uuid::new_v4().to_string(),
            status: ActionStatus::PendingHumanApproval,
            args: RunCommandArgs {
                description: "Initialize Terraform working directory".to_string(),
                reasoning: "Need to initialize Terraform before we can create a plan".to_string(),
                command: format!("terraform {} init", dir_arg),
                rollback_command: None,
            },
            exit_code: None,
            output: None,
        },
        Action::RunCommand {
            id: Uuid::new_v4().to_string(),
            status: ActionStatus::PendingHumanApproval,
            args: RunCommandArgs {
                description: "Create Terraform plan".to_string(),
                reasoning: "Generate execution plan to preview changes".to_string(),
                command: format!("terraform {} plan -out=tfplan", dir_arg),
                rollback_command: None,
            },
            exit_code: None,
            output: None,
        },
        Action::RunCommand {
            id: Uuid::new_v4().to_string(),
            status: ActionStatus::PendingHumanApproval,
            args: RunCommandArgs {
                description: "Apply Terraform plan".to_string(),
                reasoning: "Apply the reviewed Terraform plan to create/update infrastructure"
                    .to_string(),
                command: format!("terraform {} apply -auto-approve tfplan", dir_arg),
                rollback_command: Some(format!("terraform {} destroy -auto-approve", dir_arg)),
            },
            exit_code: None,
            output: None,
        },
    ];

    let agent_id = AgentID::KevinV1;
    let input = AgentInput::KevinV1 {
        user_prompt: Some("apply my Terrafrom code".into()),
        action_queue: Some(action_queue),
        action_history: None,
        scratchpad: Box::new(None),
    };

    run_agent(config, client, agent_id, None, Some(input), true).await
}

pub async fn run_dockerfile_agent(
    config: &AppConfig,
    client: &Client,
    dir: Option<String>,
) -> Result<Uuid, String> {
    let dir = dir.unwrap_or(".".into());

    let action_queue = vec![Action::RunCommand {
        id: Uuid::new_v4().to_string(),
        status: ActionStatus::PendingHumanApproval,
        args: RunCommandArgs {
            description: "Build Docker image".to_string(),
            reasoning: "Build container image from Dockerfile in the specified directory"
                .to_string(),
            command: format!("docker build -f {}/Dockerfile {}", dir, dir),
            rollback_command: None,
        },
        exit_code: None,
        output: None,
    }];

    let agent_id = AgentID::KevinV1;
    let input = AgentInput::KevinV1 {
        user_prompt: Some("build my Dockerfile".into()),
        action_queue: Some(action_queue),
        action_history: None,
        scratchpad: Box::new(None),
    };

    run_agent(config, client, agent_id, None, Some(input), true).await
}

pub async fn run_kubernetes_agent(
    config: &AppConfig,
    client: &Client,
    documents: &[PathBuf],
) -> Result<Uuid, String> {
    let action_queue = vec![Action::RunCommand {
        id: Uuid::new_v4().to_string(),
        status: ActionStatus::PendingHumanApproval,
        args: RunCommandArgs {
            description: "Apply Kubernetes manifests".to_string(),
            reasoning:
                "Apply Kubernetes configuration files to create/update resources in the cluster"
                    .to_string(),
            command: documents
                .iter()
                .map(|p| format!("kubectl apply -f {}", p.display()))
                .collect::<Vec<_>>()
                .join(" && "),
            rollback_command: Some(
                documents
                    .iter()
                    .map(|p| format!("kubectl delete -f {}", p.display()))
                    .collect::<Vec<_>>()
                    .join(" && "),
            ),
        },
        exit_code: None,
        output: None,
    }];

    let agent_id = AgentID::KevinV1;
    let input = AgentInput::KevinV1 {
        user_prompt: Some("apply my Kubernetes manifests".into()),
        action_queue: Some(action_queue),
        action_history: None,
        scratchpad: Box::new(None),
    };

    run_agent(config, client, agent_id, None, Some(input), true).await
}
