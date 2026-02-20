use rand::Rng;

pub fn truncate_output(output: &str) -> String {
    const MAX_OUTPUT_LENGTH: usize = 4000;
    // Truncate long output
    if output.len() > MAX_OUTPUT_LENGTH {
        let offset = MAX_OUTPUT_LENGTH / 2;
        let start = output
            .char_indices()
            .nth(offset)
            .map(|(i, _)| i)
            .unwrap_or(output.len());
        let end = output
            .char_indices()
            .rev()
            .nth(offset)
            .map(|(i, _)| i)
            .unwrap_or(0);

        // start/end from char_indices() — always valid char boundaries
        #[allow(clippy::string_slice)]
        return format!("{}\n...truncated...\n{}", &output[..start], &output[end..]);
    }

    output.to_string()
}

pub fn generate_simple_id(length: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();

    (0..length)
        .map(|_| {
            let idx = rng.random_range(0..CHARS.len());
            CHARS[idx] as char
        })
        .collect()
}
