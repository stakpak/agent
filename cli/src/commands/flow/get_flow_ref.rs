use crate::client::{models::FlowRef, Client};

pub async fn get_flow_ref(client: &Client, flow_ref: String) -> Result<FlowRef, String> {
    let parts: Vec<&str> = flow_ref.split('/').collect();

    Ok(match parts.len() {
        3 => FlowRef::Version {
            owner_name: parts[0].to_string(),
            flow_name: parts[1].to_string(),
            version_id: parts[2].to_string(),
        },
        2 => {
            let owner_name = parts[0];
            let flow_name = parts[1];

            let res = client.get_flow(owner_name, flow_name).await?;

            let latest_version = res
                .resource
                .versions
                .iter()
                .max_by_key(|v| v.created_at)
                .ok_or("No versions found")?;

            FlowRef::Version {
                owner_name: owner_name.to_string(),
                flow_name: flow_name.to_string(),
                version_id: latest_version.id.to_string(),
            }
        }
        _ => FlowRef::new(flow_ref).map_err(|e| format!("Failed to parse flow ref: {}", e))?,
    })
}
