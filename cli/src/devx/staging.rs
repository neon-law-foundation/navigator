//! The real cluster behind the staging lifecycle.
//!
//! This is the adapter and the wiring: [`Kubectl`] shells out, and
//! [`dispatch_kind`] composes the guarded operations with the KIND
//! orchestration (`super::up`/`super::status`) that only a live cluster can
//! run. Every
//! decision — what argv runs, in what order, and whether a refused target is
//! left untouched — lives in [`super::lifecycle`], which is NOT ignored and
//! proves all of it against a fake. Ignored by the coverage gate in `ci.yml`
//! because what remains here cannot execute without a cluster.

use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use store::DeploymentEnvironment;

use super::lifecycle::{
    ensure_namespace, fixture_is_reusable, inspect, kind_boundary_argv, new_environment_id, reset,
    stamp, teardown, wait_namespace_absent, Action, Cluster, NAMESPACE_DELETE_ATTEMPTS,
};

/// The real cluster: every method shells out to `kubectl`.
struct Kubectl;

impl Cluster for Kubectl {
    fn run(&mut self, argv: &[String]) -> Result<()> {
        let status = Command::new("kubectl")
            .args(argv)
            .status()
            .context("run kubectl staging lifecycle")?;
        if status.success() {
            Ok(())
        } else {
            bail!("kubectl staging lifecycle failed with {status}")
        }
    }

    fn succeeds(&mut self, argv: &[String]) -> Result<bool> {
        Ok(Command::new("kubectl")
            .args(argv)
            .status()
            .context("probe kubectl staging lifecycle")?
            .success())
    }

    fn capture(&mut self, argv: &[String]) -> Result<Vec<u8>> {
        let output = Command::new("kubectl")
            .args(argv)
            .output()
            .context("inspect staging namespace")?;
        if !output.status.success() {
            bail!(
                "inspect staging namespace failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output.stdout)
    }

    fn pause(&mut self) {
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn environment() -> Result<DeploymentEnvironment> {
    DeploymentEnvironment::from_env().context("parse NAVIGATOR_ENVIRONMENT")
}

pub(super) fn dispatch_kind(action: Action, cfg: &super::KindConfig) -> Result<()> {
    let env = environment()?;
    let context = cfg.kind_context();
    let namespace = cfg.namespace.clone();
    let cluster = &mut Kubectl;
    let boundary = kind_boundary_argv(&context, &namespace);

    match action {
        Action::Up => {
            bring_up(cluster, cfg, &context, &namespace)?;
            let id = new_environment_id();
            stamp(cluster, env, &context, &namespace, &id)?;
            announce(&context, &namespace, &id);
            Ok(())
        }
        Action::Reset => {
            let id = new_environment_id();
            reset(
                cluster,
                env,
                &context,
                &namespace,
                &boundary,
                &id,
                |cluster| {
                    // Namespace deletion closes the old forwards; recreate every
                    // dependency, Garage credential, and worker from the same
                    // renderer.
                    wait_namespace_absent(
                        cluster,
                        &context,
                        &namespace,
                        NAMESPACE_DELETE_ATTEMPTS,
                    )?;
                    ensure_namespace(cluster, &context, &namespace)?;
                    super::up(cfg)
                },
            )?;
            announce(&context, &namespace, &id);
            Ok(())
        }
        Action::Status => {
            super::status(cfg);
            let target = inspect(cluster, env, &context, &namespace)?;
            println!("staging: environment-id={}", target.environment_id);
            Ok(())
        }
        Action::Down => teardown(cluster, env, &context, &namespace, &boundary),
    }
}

fn bring_up(
    cluster: &mut Kubectl,
    cfg: &super::KindConfig,
    context: &str,
    namespace: &str,
) -> Result<()> {
    // A live port-forward is the cheap first signal that the tier is
    // actually reachable, not merely present.
    let port_answers = std::net::TcpStream::connect(("127.0.0.1", cfg.surreal_port)).is_ok();
    if fixture_is_reusable(cluster, context, namespace, port_answers)? {
        eprintln!("==> reusing the reachable KIND staging fixture");
        return Ok(());
    }
    ensure_namespace(cluster, context, namespace)?;
    super::up(cfg)
}

fn announce(context: &str, namespace: &str, id: &str) {
    println!("staging: context={context} namespace={namespace} environment-id={id}");
}
