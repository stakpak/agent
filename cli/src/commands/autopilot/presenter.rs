use super::probes::{ProbeResult, ProbeSeverity, ProbeStatus, Remediation, summarize};

pub fn print_probe_report(title: &str, results: &[ProbeResult]) {
    println!("{title}");

    for result in results {
        print_probe_result(result);
    }

    let summary = summarize(results);
    println!();
    println!(
        "Summary: {} blocking, {} warning, {} passing, {} skipped",
        summary.blocking_failures, summary.warnings, summary.passes, summary.skipped
    );
    println!();
}

fn print_probe_result(result: &ProbeResult) {
    println!("{} {}", status_icon(result), result.summary);

    if let Some(details) = &result.details {
        println!("  {details}");
    }

    if let Some(remediation) = &result.remediation {
        match remediation {
            Remediation::Manual { summary, command } => {
                println!("  Fix: {summary}");
                if let Some(command) = command {
                    println!("  Run: {command}");
                }
            }
            Remediation::Suggested { summary } => {
                println!("  Suggestion: {summary}");
            }
        }
    }
}

fn status_icon(result: &ProbeResult) -> &'static str {
    match result.status {
        ProbeStatus::Pass => "✓",
        ProbeStatus::Skip => "-",
        ProbeStatus::Fail => match result.severity {
            ProbeSeverity::Blocking => "✗",
            ProbeSeverity::Warning => "⚠",
            ProbeSeverity::Info => "i",
        },
    }
}
