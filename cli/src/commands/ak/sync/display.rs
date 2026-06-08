use stakpak_shared::format::{format_size, short_hash};

use super::SyncDirection;
use super::execute::SyncReport;
use super::plan::{Conflict, SyncPlan};

pub fn print_plan(p: &SyncPlan) {
    let direction_label = match p.direction {
        SyncDirection::Push => "push",
        SyncDirection::Pull => "pull",
    };
    println!(
        "ak sync {direction_label} (dry-run): {} upload(s), {} download(s), {} skipped, {} conflict(s)",
        p.uploads.len(),
        p.downloads.len(),
        p.skipped.len(),
        p.conflicts.len()
    );

    if !p.uploads.is_empty() {
        println!();
        println!("uploads:");
        for meta in &p.uploads {
            println!("  + {}  ({})", meta.path, format_size(meta.size_bytes));
        }
    }
    if !p.downloads.is_empty() {
        println!();
        println!("downloads:");
        for meta in &p.downloads {
            println!("  + {}  ({})", meta.path, format_size(meta.size_bytes));
        }
    }
    if !p.skipped.is_empty() {
        println!();
        println!("unchanged ({}):", p.skipped.len());
        for path in &p.skipped {
            println!("  = {path}");
        }
    }
    if !p.conflicts.is_empty() {
        println!();
        println!("conflicts:");
        for c in &p.conflicts {
            println!(
                "  ! {}  local: {} {}  remote: {} {}",
                c.path,
                short_hash(&c.local_hash),
                format_size(c.local_size),
                short_hash(&c.remote_hash),
                format_size(c.remote_size),
            );
        }
    }
}

pub fn print_conflicts_to_stderr(conflicts: &[Conflict]) {
    for c in conflicts {
        eprintln!(
            "  ! {}  local: {} {}  remote: {} {}",
            c.path,
            short_hash(&c.local_hash),
            format_size(c.local_size),
            short_hash(&c.remote_hash),
            format_size(c.remote_size),
        );
    }
}

pub fn print_report(r: &SyncReport) {
    let direction_label = match r.direction {
        SyncDirection::Push => "push",
        SyncDirection::Pull => "pull",
    };

    let total_changed = r.uploaded.len() + r.downloaded.len() + r.conflict_resolved.len();
    println!(
        "ak sync {direction_label}: {total_changed} change(s), {} skipped, {} failure(s)",
        r.skipped.len(),
        r.failures.len(),
    );

    if !r.uploaded.is_empty() {
        println!();
        println!("uploaded:");
        for path in &r.uploaded {
            println!("  + {path}");
        }
    }
    if !r.downloaded.is_empty() {
        println!();
        println!("downloaded:");
        for path in &r.downloaded {
            println!("  + {path}");
        }
    }
    if !r.conflict_resolved.is_empty() {
        println!();
        println!("conflicts resolved:");
        for path in &r.conflict_resolved {
            println!("  ~ {path}");
        }
    }
    if !r.failures.is_empty() {
        // Use stderr for failures so a successful pipe (e.g. `... | tee
        // sync.log`) still surfaces them prominently.
        eprintln!();
        eprintln!("failures:");
        for f in &r.failures {
            eprintln!("  ✗ {}: {}", f.path, f.error);
        }
    }
}
