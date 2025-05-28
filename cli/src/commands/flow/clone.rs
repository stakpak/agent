use std::{collections::HashMap, path::PathBuf};

use stakpak_api::{
    Client,
    models::{FlowRef, ProvisionerType},
};

pub async fn clone(
    client: &Client,
    flow_ref: &FlowRef,
    dir: Option<&str>,
) -> Result<HashMap<ProvisionerType, Vec<PathBuf>>, String> {
    let documents = client.get_flow_documents(flow_ref).await?;
    let base_dir = dir.unwrap_or(".");

    let mut path_map = HashMap::new();

    for doc in documents
        .documents
        .into_iter()
        .chain(documents.additional_documents)
    {
        let path = doc.uri.strip_prefix("file:///").unwrap_or(&doc.uri);
        let full_path = std::path::Path::new(&base_dir).join(path);

        path_map
            .entry(doc.provisioner)
            .or_insert_with(Vec::new)
            .push(full_path.clone());

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
        }

        // Write the files
        std::fs::write(&full_path, doc.content)
            .map_err(|e| format!("Failed to write file {}: {}", full_path.display(), e))?;

        println!("Cloned {} -> \"{}\"", doc.uri, full_path.display());
    }

    println!("Successfully cloned flow to \"{}\"", base_dir);

    Ok(path_map)
}
