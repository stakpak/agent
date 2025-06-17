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

        return format!("{}\n...truncated...\n{}", &output[..start], &output[end..]);
    }

    output.to_string()
}
