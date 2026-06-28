use std::path::Path;
use std::process::ExitCode;

/// Validate the event markdown under `dir`.
///
/// Events are linted by the **shared** rules engine — the same engine
/// that lints notation templates and prose markdown — which classifies
/// each file and applies the E-family event rules (timestamp + timezone,
/// the event/template mutual-exclusivity rule, and location-or-meeting).
/// On top of the lint pass we still run the typed loader, which performs
/// the deeper semantic checks the lint rules deliberately leave out (the
/// timestamp parses, `ends_at` is after `starts_at`, the timezone is one
/// we emit a `VTIMEZONE` for, and the filename date matches `starts_at`).
pub fn run_validate(dir: &Path) -> ExitCode {
    let mut failed = false;

    // 1. Shared rules engine: classify every file and lint it.
    let engine = rules::ClassifiedRuleEngine::new();
    match engine.lint_directory(dir) {
        Ok(report) => {
            for v in &report.violations {
                eprintln!("{}:{}: {} {}", v.path.display(), v.line, v.code, v.message);
            }
            if !report.is_clean() {
                failed = true;
            }
        }
        Err(err) => {
            eprintln!("navigator: {err}");
            return ExitCode::from(2);
        }
    }

    // 2. Typed loader: the deep semantic checks beyond the lint contract.
    match web::events::load_dir(dir) {
        Ok(index) => {
            if !failed {
                println!(
                    "Validated {} event markdown file(s) in {}",
                    index.events().len(),
                    dir.display()
                );
            }
        }
        Err(err) => {
            eprintln!("navigator: {err}");
            failed = true;
        }
    }

    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
