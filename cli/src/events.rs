use std::path::Path;
use std::process::ExitCode;

pub fn run_validate(dir: &Path) -> ExitCode {
    match web::events::load_dir(dir) {
        Ok(index) => {
            println!(
                "Validated {} event markdown file(s) in {}",
                index.events().len(),
                dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("navigator: {err}");
            ExitCode::from(1)
        }
    }
}
