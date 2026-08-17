//! `navigator ops rebrand` builds and verifies a mounted brand bundle.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use views::brand_bundle::{validate_manifest, Assets, BrandBundle, BrandManifest, MANIFEST_FILE};

#[derive(Subcommand)]
pub enum BrandCmd {
    /// Build a self-contained, mountable bundle from `navigator.yaml`.
    Build {
        /// Source manifest and source-relative static files.
        #[arg(long, default_value = "navigator.yaml")]
        file: PathBuf,
        /// Empty directory receiving `navigator.yaml` and brand files.
        #[arg(long, default_value = ".devx/brand-bundle")]
        out: PathBuf,
    },
    /// Verify a mountable bundle without changing it.
    Verify {
        /// Bundle directory (containing `navigator.yaml`).
        #[arg(long, default_value = ".devx/brand-bundle")]
        dir: PathBuf,
    },
}

pub fn run(cmd: BrandCmd) -> Result<()> {
    match cmd {
        BrandCmd::Build { file, out } => build(&file, &out),
        BrandCmd::Verify { dir } => verify(&dir),
    }
}

/// Primary public domain for deployment commands (`ops ship`, Restate
/// re-register). Branding is Neon Law by default, so this resolves to
/// [`views::brand::DEFAULT_BRANDING`]'s `neonlaw.com` unless the deployment
/// opts into custom branding via `NAVIGATOR_CUSTOM_BRANDING`, whose bundle's
/// `brand.primary_domain` then wins (fail closed within custom mode — a fork
/// that means to rebrand can't half-rebrand). In the default identity an
/// operator may override just the domain string with `NAVIGATOR_PRIMARY_DOMAIN`
/// (e.g. a non-production host) without building a custom bundle.
pub fn primary_domain() -> Result<String> {
    primary_domain_with(|key| std::env::var(key).ok())
}

/// [`primary_domain`] over an explicit lookup instead of the process
/// environment — `ops ship` resolves it from the selected deployment's
/// `config.toml` coordinates.
pub(super) fn primary_domain_with<F>(get: F) -> Result<String>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(bundle) = BrandBundle::from_env_with(&get).map_err(anyhow::Error::new)? {
        return bundle
            .manifest
            .brand
            .primary_domain
            .filter(|domain| !domain.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "brand.primary_domain must be set in the {} brand bundle for deployment commands",
                    views::brand_bundle::CUSTOM_BRANDING_ENV
                )
            });
    }
    Ok(get("NAVIGATOR_PRIMARY_DOMAIN")
        .filter(|domain| !domain.trim().is_empty())
        .unwrap_or_else(|| views::brand::DEFAULT_BRANDING.primary_domain.to_owned()))
}

fn build(file: &Path, out: &Path) -> Result<()> {
    let raw = fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let manifest: BrandManifest =
        serde_yaml::from_str(&raw).with_context(|| format!("parsing {}", file.display()))?;
    let source_root = file.parent().unwrap_or_else(|| Path::new("."));
    validate_manifest(&manifest, source_root).map_err(anyhow::Error::new)?;
    if out.exists() {
        bail!(
            "output bundle {} already exists; choose a new empty path",
            out.display()
        );
    }
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    copy_assets(&manifest.assets, source_root, out)?;
    let rendered = serde_yaml::to_string(&manifest).context("serializing brand manifest")?;
    fs::write(out.join(MANIFEST_FILE), rendered)
        .with_context(|| format!("writing {}/{}", out.display(), MANIFEST_FILE))?;
    BrandBundle::load(out).map_err(anyhow::Error::new)?;
    println!("built mountable brand bundle → {}", out.display());
    Ok(())
}

fn copy_assets(assets: &Assets, source_root: &Path, out: &Path) -> Result<()> {
    for (field, path) in assets.entries() {
        let source = source_root.join(path);
        let target = out.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::copy(&source, &target).with_context(|| {
            format!(
                "copying {field}: {} → {}",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

fn verify(dir: &Path) -> Result<()> {
    BrandBundle::load(dir).map_err(anyhow::Error::new)?;
    println!("✓ {} is a valid mounted brand bundle", dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn build_writes_a_valid_self_contained_bundle() {
        let source = tempdir().unwrap();
        fs::write(source.path().join("firm.svg"), "svg").unwrap();
        fs::create_dir(source.path().join("theme")).unwrap();
        fs::write(
            source.path().join("theme/accent.css"),
            "body { color: teal; }",
        )
        .unwrap();
        fs::write(
            source.path().join("navigator.yaml"),
            "version: 1\nassets:\n  firm_logo: firm.svg\n  static_files:\n    accent.css: theme/accent.css\n",
        )
        .unwrap();
        let output = tempdir().unwrap().path().join("bundle");
        run(BrandCmd::Build {
            file: source.path().join("navigator.yaml"),
            out: output.clone(),
        })
        .unwrap();
        assert!(output.join("firm.svg").is_file());
        assert_eq!(
            fs::read_to_string(output.join("theme/accent.css")).unwrap(),
            "body { color: teal; }"
        );
        run(BrandCmd::Verify {
            dir: output.clone(),
        })
        .unwrap();

        let err = build(&source.path().join("navigator.yaml"), &output).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn primary_domain_reads_the_process_environment() {
        // Exercises the real `primary_domain` wrapper over `std::env`; the
        // resolved domain is always a non-empty host (Neon Law by default).
        assert!(!primary_domain().unwrap().is_empty());
    }

    #[test]
    fn primary_domain_defaults_to_neon_law() {
        // No custom branding, no override → the built-in Neon Law identity.
        assert_eq!(
            primary_domain_with(|_| None).unwrap(),
            views::brand::DEFAULT_BRANDING.primary_domain
        );
        assert_eq!(primary_domain_with(|_| None).unwrap(), "neonlaw.com");
    }

    #[test]
    fn primary_domain_override_applies_in_default_identity() {
        // NAVIGATOR_PRIMARY_DOMAIN overrides just the domain string without a
        // custom bundle; blank is ignored (falls back to the default).
        let resolved = primary_domain_with(|key| {
            (key == "NAVIGATOR_PRIMARY_DOMAIN").then(|| "staging.example".to_string())
        })
        .unwrap();
        assert_eq!(resolved, "staging.example");
        let blank = primary_domain_with(|key| {
            (key == "NAVIGATOR_PRIMARY_DOMAIN").then(|| "  ".to_string())
        })
        .unwrap();
        assert_eq!(blank, "neonlaw.com");
    }

    #[test]
    fn custom_branding_bundle_domain_wins() {
        let bundle = tempdir().unwrap();
        fs::write(
            bundle.path().join(MANIFEST_FILE),
            "version: 1\nbrand:\n  primary_domain: acme.example\n",
        )
        .unwrap();
        let path = bundle.path().to_string_lossy().into_owned();
        let resolved = primary_domain_with(|key| {
            (key == views::brand_bundle::CUSTOM_BRANDING_ENV).then(|| path.clone())
        })
        .unwrap();
        assert_eq!(resolved, "acme.example");
    }

    #[test]
    fn custom_branding_without_a_domain_is_an_error() {
        let bundle = tempdir().unwrap();
        fs::write(bundle.path().join(MANIFEST_FILE), "version: 1\n").unwrap();
        let path = bundle.path().to_string_lossy().into_owned();
        let err = primary_domain_with(|key| {
            (key == views::brand_bundle::CUSTOM_BRANDING_ENV).then(|| path.clone())
        })
        .unwrap_err();
        assert!(err.to_string().contains("brand.primary_domain"));
    }
}
