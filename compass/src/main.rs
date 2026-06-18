//! Compass — a downstream consumer of the `rules` crate.
//!
//! Compass currently runs the same default rule set as `navigator`,
//! plus its own additions. Today the only extra rule is `C001`
//! (every linted file must end with a non-empty body line). The point
//! of the slice is to prove the `rules` crate composes cleanly from
//! a separate binary so Compass-specific rules can grow alongside
//! Navigator's without forking the engine.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rules::{Rule, SourceFile, Violation};

#[derive(Parser)]
#[command(
    name = "compass",
    version,
    about = "Compass markdown notation validator"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate markdown files under <dir> with the Compass rule set
    /// (Navigator defaults plus Compass-specific C-rules).
    Validate {
        dir: PathBuf,
        /// Skip Navigator's F-family rules (frontmatter / Navigator
        /// notation specific) and only run general Markdown checks
        /// alongside Compass-specific rules.
        #[arg(long)]
        markdown_only: bool,
    },
}

/// `C001` — every file must end with a non-blank body line.
struct C001RequireBody;

impl C001RequireBody {
    const CODE: &'static str = "C001";
}

impl Rule for C001RequireBody {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let body =
            rules::frontmatter::extract(&file.contents).map_or(file.contents.as_str(), |_| {
                // Skip past the frontmatter and its closing `---` line.
                file.contents
                    .splitn(3, "---\n")
                    .nth(2)
                    .unwrap_or("")
                    .trim_start_matches('\n')
            });
        let last_non_blank = body.lines().rev().find(|l| !l.trim().is_empty());
        if last_non_blank.is_some() {
            Vec::new()
        } else {
            vec![Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: 1,
                range: rules::line_byte_range(&file.contents, 1),
                message: "Compass requires a non-empty body after frontmatter".to_string(),
            }]
        }
    }
}

fn compass_rules(markdown_only: bool) -> Vec<Box<dyn Rule>> {
    let mut all = if markdown_only {
        rules::navigator_markdown_only_rules()
    } else {
        rules::navigator_default_rules()
    };
    all.push(Box::new(C001RequireBody));
    all
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Validate { dir, markdown_only } => run_validate(&dir, markdown_only),
    }
}

fn run_validate(dir: &std::path::Path, markdown_only: bool) -> ExitCode {
    let engine = rules::RuleEngine::new(compass_rules(markdown_only));
    let report = match engine.lint_directory(dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("compass: {e}");
            return ExitCode::from(2);
        }
    };
    for v in &report.violations {
        println!("{}:{} {}: {}", v.path.display(), v.line, v.code, v.message);
    }
    println!(
        "Scanned {} file(s), found {} violation(s)",
        report.files_scanned,
        report.violations.len()
    );
    if report.is_clean() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
