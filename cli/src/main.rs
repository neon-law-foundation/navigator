use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod assets;
mod credentials;
mod devx;
mod drive;
mod erd;
mod events;
mod format;
mod forms_sync;
mod git;
mod glossary;
mod import;
mod intake;
mod list;
mod login;
mod lsp_publish;
mod palette;
mod project;
mod remote;
mod scaffold;
mod transcribe;

use devx::brand::BrandCmd;
use devx::{DnsCmd, GcpCmd, RestateCmd};

/// The version `navigator --version` / `-V` reports.
///
/// Precedence, highest first:
/// 1. A runtime `NAVIGATOR_RELEASE_TAG` — the workspace-wide convention `web`
///    and `lsp` already follow, and the seam tests assert against.
/// 2. The tag baked at build time by `build.rs` (`NAVIGATOR_CLI_VERSION`), so a
///    *downloaded* release binary self-reports its `YY.MM.DD` release with no
///    environment set.
/// 3. The workspace crate version (`0.1.0`) on a plain local build, since
///    `build.rs` falls back to `CARGO_PKG_VERSION` when no tag is present.
fn cli_version() -> &'static str {
    if let Ok(tag) = std::env::var("NAVIGATOR_RELEASE_TAG") {
        let tag = tag.trim();
        if !tag.is_empty() {
            // Leak the single resolved version string: it lives for the whole
            // process, and clap's `version` wants a `&'static str`.
            return Box::leak(tag.to_owned().into_boxed_str());
        }
    }
    env!("NAVIGATOR_CLI_VERSION")
}

#[derive(Parser)]
#[command(
    name = "navigator",
    version = cli_version(),
    about = "Neon Law Navigator CLI — notation validator/importer + live-site matter driver",
    long_about = "Neon Law Navigator CLI — notation validator/importer + live-site matter driver\n\nNothing here is legal advice. Neon Law Navigator validates and moves legal notation, but an attorney remains responsible for legal advice and judgment."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate every `.md` under `<dir>` against the classified
    /// Neon Law Navigator rule set. DB-free by default: notation templates get
    /// N-family structural checks, while prose markdown gets only
    /// Markdown rules. Pass `--database-url` (or set `DATABASE_URL`) to
    /// load the canonical question registry and have N104 reject
    /// unknown codes.
    Validate {
        /// Directory to walk.
        dir: PathBuf,
        /// Skip the N-family rules (Neon Law Navigator notation-template
        /// specific) and only run general Markdown checks.
        #[arg(long)]
        markdown_only: bool,
        /// Validate files normally skipped by name (`README.md`,
        /// `CLAUDE.md`, `CODE_OF_CONDUCT.md`, `LICENSE.md`, `ERD.md`)
        /// and directories (`AgentDocumentation`, `workshops`,
        /// `Blog`).
        #[arg(long)]
        no_default_excludes: bool,
        /// Apply every safe-by-construction rule autofix
        /// (whitespace, ATX heading spacing, blockquote spacing) to
        /// the files in place, then re-validate. Diagnostic-only
        /// rules (N-family notation-template, M024 duplicate headings,
        /// M026 trailing punctuation) are still reported but not
        /// auto-fixed. The autofixed-source view is what the
        /// `navigator-lsp` `source.fixAll` action ships in editors.
        #[arg(long)]
        fix: bool,
        /// Postgres connection URL. When provided, loads the
        /// canonical question-code registry so N104 can reject
        /// unknown codes. Falls back to the `DATABASE_URL`
        /// environment variable.
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,
    },
    /// Validate reviewable event markdown under `<dir>`.
    ///
    /// Event files mirror the blog convention (`YYYYMMDD_slug.md`) and
    /// require structured front matter for title, description, local
    /// Pacific start/end times, place, current external event provider,
    /// and optional post-event video/recap links.
    ValidateEvents {
        /// Directory to walk.
        #[arg(default_value = "web/content/events")]
        dir: PathBuf,
    },
    /// Render a single notation template to a PDF, framed by an output
    /// format (a plain document, or a firm `letter` on Neon Law
    /// letterhead with the logo).
    ///
    /// The file is validated against the same notation rule set as
    /// `validate` first — a template with any violation is refused. The
    /// output format is taken from the template's `output:` frontmatter
    /// field, overridable with `--format`; absent both, it renders
    /// plain. Markdown is converted to Typst and compiled in pure Rust
    /// (no shell-out). `{{placeholder}}` tokens render verbatim unless
    /// filled with `--answer code=value`.
    Render {
        /// Path to the notation template (`.md`).
        file: PathBuf,
        /// Where to write the rendered PDF.
        #[arg(long)]
        out: PathBuf,
        /// Output format (`plain` or `letter`). Overrides the
        /// template's `output:` frontmatter field when set.
        #[arg(long)]
        format: Option<String>,
        /// Fill a `{{code}}` placeholder with `value`. Repeatable:
        /// `--answer counterparty_legal_name="NEON GmbH"`.
        #[arg(long = "answer", value_parser = parse_answer)]
        answers: Vec<(String, String)>,
    },
    /// Import every clean template under `<dir>` into a Postgres
    /// database.
    ///
    /// Each markdown file's frontmatter becomes a `templates` row;
    /// every question code referenced by the `questionnaire:` and
    /// `workflow:` state maps becomes a `questions` row
    /// (auto-imported if not yet known). Files with any rule
    /// violation are skipped so the database only ever holds clean
    /// inputs.
    Import {
        /// Directory to walk.
        dir: PathBuf,
        /// Postgres connection URL. Falls back to the
        /// `DATABASE_URL` environment variable; errors when neither
        /// is set.
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
    /// Bulk-import a contacts file (JSON or YAML) of organizations and
    /// the people who work at them — find-or-create `entities`,
    /// `persons`, and the links between them. Idempotent; safe to
    /// re-run. See `docs/bulk-contact-import.md`.
    ImportContacts {
        /// Path to the contacts file (`.json` or `.yaml`).
        file: PathBuf,
        /// Validate and report without writing to the database.
        #[arg(long)]
        dry_run: bool,
        /// Postgres connection URL. Falls back to the `DATABASE_URL`
        /// environment variable; errors when neither is set.
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
    /// List rows from a Postgres database, after running the full
    /// canonical seed pass. The seed is idempotent so re-running
    /// list against an already-populated database is safe.
    List {
        #[command(subcommand)]
        subject: ListSubject,
        /// Postgres connection URL. Falls back to the
        /// `DATABASE_URL` environment variable.
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
    /// Print an ERD describing every table in the migrated schema.
    /// Default format is a Mermaid `erDiagram` block; `--format svg`
    /// emits a deterministic, hand-written SVG (suitable for piping
    /// into `docs/erd.svg`). Introspects Postgres `pg_catalog` /
    /// `information_schema`.
    Erd {
        /// Postgres connection URL. Falls back to the
        /// `DATABASE_URL` environment variable. The database is
        /// migrated (idempotently) before introspection — point at
        /// a throwaway database if you don't want migrations applied
        /// in place.
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        /// Output format. `mermaid` (default) → GitHub-renderable
        /// `erDiagram` block. `svg` → deterministic standalone SVG.
        #[arg(long, value_enum, default_value_t = erd::OutputFormat::Mermaid)]
        format: erd::OutputFormat,
    },
    /// Normalize whitespace and bullet style in a Markdown notation.
    /// Frontmatter passes through untouched; the body has `- `
    /// bullets converted to `* ` and trailing spaces stripped.
    Format {
        /// File to format in place.
        file: PathBuf,
    },
    /// Print canonical Neon Law Navigator vocabulary. With no argument lists
    /// every term; with one argument prints just that term (case-
    /// insensitive), exiting non-zero on a miss.
    Glossary {
        /// Optional term to look up.
        term: Option<String>,
    },
    /// Transcode curated source photos into responsive web variants.
    Assets {
        #[command(subcommand)]
        action: AssetsAction,
    },
    /// Vendored government forms (`notation_templates/forms/` + FORMS.toml).
    Forms {
        #[command(subcommand)]
        action: FormsAction,
    },
    /// Distribute the `navigator-lsp` editor binary.
    Lsp {
        #[command(subcommand)]
        action: LspAction,
    },
    /// Write-side operations against the `projects` table.
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Per-Project git repo operations. `token` mints a Personal
    /// Access Token a `git` CLI presents as HTTP Basic; `url` prints a
    /// Project's clone URL (`<base>/projects/<id>.git`).
    Git {
        #[command(subcommand)]
        action: GitAction,
    },
    /// Google Drive operations. `login` mints + persists a refresh
    /// token at `~/.config/navigator/drive_token.json` via the
    /// installed-app OAuth flow. `ls` lists shared drives (no args)
    /// or the contents of a folder (`--drive <id> [--folder <id>]`).
    Drive {
        #[command(subcommand)]
        action: DriveAction,
    },
    /// Authenticate to a live Neon Law Navigator site via a browser-loopback
    /// flow and store a short-lived (~8h) bearer token at
    /// `~/.navigator.json` (mode `0600`). Like `gcloud auth login`: opens
    /// the browser, reuses the existing OIDC session, and lands the token
    /// on a loopback listener.
    Login {
        /// Host to authenticate to, e.g. `www.neonlaw.com`. A bare host
        /// gets `https://`; pass a full URL (e.g.
        /// `http://localhost:8080`) to target a local cluster.
        #[arg(long)]
        host: String,
    },
    /// Forget the stored token for a host (or the sole logged-in host).
    Logout {
        /// Host to log out of. Optional when exactly one host is stored.
        #[arg(long)]
        host: Option<String>,
    },
    /// Print the stored identity and how long the token has left.
    Whoami {
        /// Host to inspect. Optional when exactly one host is stored.
        #[arg(long)]
        host: Option<String>,
    },
    /// Read-side operations against a live site's matters.
    Projects {
        #[command(subcommand)]
        action: ProjectsAction,
    },
    /// Drive a retainer notation on a live site.
    Retainer {
        #[command(subcommand)]
        action: RetainerAction,
    },
    /// Open a questionnaire-driven matter on a live site.
    Matter {
        #[command(subcommand)]
        action: MatterAction,
    },
    /// Open and list recurring-billing subscriptions on a live site.
    Subscription {
        #[command(subcommand)]
        action: SubscriptionAction,
    },
    /// Mint and list reusable discount coupons on a live site.
    Coupon {
        #[command(subcommand)]
        action: CouponAction,
    },
    /// Answer a notation's intake questionnaire from the terminal.
    Intake {
        #[command(subcommand)]
        action: IntakeAction,
    },
    /// Inspect or drive a notation's workflow on a live site.
    Notation {
        #[command(subcommand)]
        action: NotationAction,
    },
    /// Drop the three files that a new legal workflow starts with:
    /// `notation_templates/<category>/<jurisdiction>.md`,
    /// `workflows/specs/<code>.yaml`, and
    /// `features/tests/features/<matter>.feature`. Idempotent —
    /// existing files are left alone.
    Scaffold {
        /// Snake-case matter slug, e.g. `incorporation`,
        /// `estate_planning`. Forms the prefix of the template `code`.
        matter: String,
        /// Directory under `notation_templates/` to drop the markdown into.
        #[arg(long)]
        category: String,
        /// Jurisdiction name (`PascalCase` for the filename,
        /// `snake_case` for the template `code`), e.g. `Nevada`.
        #[arg(long)]
        jurisdiction: String,
    },
    /// Transcribe a recording (or replay a transcript) into Inquiry
    /// Coverage JSON for a notation template questionnaire.
    ///
    /// This is the offline/upload path; real-time streaming ("live")
    /// transcription is a separate `web` feature, not a CLI command.
    Transcribe {
        /// Template markdown file whose `questionnaire:` becomes the
        /// Inquiry Set. Required — pass `--template` or set
        /// `NAVIGATOR_NOTATION_TEMPLATE`.
        #[arg(long, env = "NAVIGATOR_NOTATION_TEMPLATE")]
        template: PathBuf,
        /// Plain-text transcript to replay without calling speech-to-text.
        #[arg(long, conflicts_with = "audio")]
        transcript: Option<PathBuf>,
        /// Audio file to transcribe. By default this uses the `fake`
        /// backend (no cloud call); pass `--speech-backend google` to
        /// transcribe with real Google Speech-to-Text. Any common format
        /// works (m4a/AAC, mp3, flac, wav, ogg) — it is decoded locally.
        #[arg(long, conflicts_with = "transcript")]
        audio: Option<PathBuf>,
        /// Speech backend for `--audio`: `fake` (default, deterministic,
        /// no cloud call) or `google` (real Speech-to-Text — needs a
        /// project and credentials). Real cloud is opt-in.
        #[arg(long, env = "NAVIGATOR_SPEECH_BACKEND", default_value = "fake")]
        speech_backend: String,
        /// Google Cloud project for Speech-to-Text.
        #[arg(long, env = "GOOGLE_CLOUD_PROJECT")]
        google_project: Option<String>,
        /// Google Speech-to-Text v2 location.
        #[arg(long, default_value = "global")]
        google_location: String,
        /// BCP-47 language code for the audio.
        #[arg(long, default_value = "en-US")]
        google_language: String,
        /// Google Speech-to-Text recognition model.
        #[arg(long, default_value = "latest_long")]
        google_model: String,
        /// Pretty-print the JSON output.
        #[arg(long)]
        pretty: bool,
    },

    // ── Local-development + deploy orchestration ──────────────────────
    // Collapsed in from the former `devx` binary. Implementations live in
    // `cli::devx`; `main` routes the whole group to `devx::dispatch`.
    /// Bring up the KIND dependency stack (Postgres, Keycloak, fake-gcs,
    /// OPA, Restate, Grafana LGTM), open host port-forwards, and write
    /// `.devx/env` — the developer-loop entry point for editing `web` on
    /// the host. Aliased as `up`.
    #[command(alias = "up")]
    StartDevServer,
    /// Kill the port-forwards and delete the KIND cluster.
    Down,
    /// Print env vars (one KEY=VALUE per line) for a host-side `web`.
    Env,
    /// Show whether the cluster and port-forwards are up.
    Status,
    /// Create the KIND cluster and install nginx-ingress + the
    /// Restate Operator. Does not apply any application manifests.
    /// Use this when you want the cluster prepared but plan to apply
    /// k8s/ manifests by hand.
    KindUp,
    /// Delete the KIND cluster. Does not touch the local Docker
    /// images or the host port-forward state file.
    KindDown,
    /// Build the `navigator-web` Docker image (`docker build -t
    /// navigator-web:dev .`). Reads `images/Dockerfile.web`.
    Image,
    /// Build the `navigator-workflows-service` worker image (reads
    /// `images/Dockerfile.workflows-service`).
    ImageWorkflowsService,
    /// Build the `navigator-archives-trigger` image from the shared
    /// `images/Dockerfile.trigger` (`--build-arg CRATE=archives`). The
    /// nightly `CronJob` at
    /// `examples/deploy/k8s/exports/cron-archives-trigger.yaml` runs this to
    /// start one `Archives` workflow invocation; the workflow itself is
    /// hosted by the `workflows-service` worker.
    ImageArchivesTrigger,
    /// Build the `navigator-statutes-trigger` image from the shared
    /// `images/Dockerfile.trigger` (`--build-arg CRATE=statutes`). The
    /// weekly `CronJob` at
    /// `examples/deploy/k8s/exports/cron-statutes-trigger.yaml` runs this to
    /// start one `Statutes` workflow invocation, which scrapes the
    /// practice-relevant NRS chapters into Postgres for the public
    /// `/statutes` reference and emails a summary.
    ImageStatutesTrigger,
    /// Build the `navigator-billing-canary-trigger` image from the shared
    /// `images/Dockerfile.trigger` (`--build-arg CRATE=billing-workflows`).
    /// Its `CronJob` starts one `BillingCanary` workflow invocation; the
    /// workflow is hosted by the `workflows-service` worker.
    ImageBillingCanaryTrigger,
    /// Build the `navigator-reconcile-invoices-trigger` image from the
    /// shared `images/Dockerfile.trigger` (`--build-arg CRATE=billing-workflows
    /// --build-arg BIN=reconcile-trigger`). Its nightly `CronJob` at
    /// `examples/deploy/k8s/exports/cron-reconcile-invoices-trigger.yaml`
    /// starts one `ReconcileInvoices` workflow invocation, which folds Xero's
    /// paid status back onto the `xero_invoices` mirror.
    ImageReconcileInvoicesTrigger,
    /// Build the `navigator-recurring-billing-trigger` image from the
    /// shared `images/Dockerfile.trigger` (`--build-arg CRATE=billing-workflows
    /// --build-arg BIN=recurring-trigger`). Its daily `CronJob` at
    /// `examples/deploy/k8s/exports/cron-recurring-trigger.yaml` starts one
    /// `RecurringBilling` workflow invocation, which raises a Xero invoice
    /// per active subscription per month (Nexus, Nautilus).
    ImageRecurringBillingTrigger,
    /// Build the `navigator-heartbeat-trigger` image from the shared
    /// `images/Dockerfile.trigger` (`--build-arg CRATE=workflows-service
    /// --build-arg BIN=heartbeat-trigger`). Its six-hourly `CronJob` at
    /// `examples/deploy/k8s/exports/cron-heartbeat-trigger.yaml` starts one
    /// `Heartbeat` workflow invocation — the durable-execution liveness canary
    /// that depends on nothing and emails firm ops a "Where to look" report.
    ImageHeartbeatTrigger,
    /// Build the `navigator-billing-digest-trigger` image from the shared
    /// `images/Dockerfile.trigger` (`--build-arg CRATE=billing-workflows
    /// --build-arg BIN=billing-digest-trigger`). Its daily `CronJob` at
    /// `examples/deploy/k8s/exports/cron-billing-digest-trigger.yaml` starts one
    /// `BillingDigest` workflow invocation — the daily GCP-cost email reporting
    /// gross/credits/net by service and the real cost when trial credits expire.
    ImageBillingDigestTrigger,
    /// Build both images, `kind load` them, then
    /// `kubectl apply -k k8s/overlays/kind` — the full stack
    /// including navigator-web. CI-shaped path: ends with the
    /// navigator-web rollout settling.
    Deploy,
    /// `kubectl delete namespace navigator`. Removes every Neon Law Navigator
    /// resource without touching the cluster itself.
    Undeploy,
    /// Smoke-test the deployed stack: wait for every rollout, hit
    /// `/health` through the ingress, assert the OPA policy decisions,
    /// and confirm the seed data populated. Native Rust — the former
    /// `scripts/e2e.sh`.
    E2e,
    /// Pre-seed the Staff demo user (`staff@neonlaw.com`) with the
    /// `staff` role so the browser e2e's admin-gated walk can run.
    /// Native Rust — the former `scripts/ci-grant-staff.sh`.
    GrantStaff,
    /// Tail `navigator-web` logs (`kubectl logs -f deployment/navigator-web`).
    Logs,
    /// `kubectl kustomize k8s/overlays/kind` — render the full local
    /// stack to stdout for inspection. Useful when debugging a
    /// kustomize overlay before applying it.
    KustomizeKind,
    /// `kubectl kustomize k8s/overlays/gke` — render the production
    /// overlay to stdout for inspection. Config Sync owns the actual
    /// apply in production; this is the local equivalent of "what
    /// will the cluster see?"
    KustomizeGke,
    /// GCP project provisioning. The actual REST plumbing lives in
    /// `cli/src/devx/gcp/`; this is the entry point operators reach for
    /// when standing up (or re-running) Neon Law Navigator on a fresh GCP
    /// project.
    #[command(subcommand)]
    Gcp(GcpCmd),
    /// Restate Cloud CLI wrappers. Saves operators from memorizing
    /// the `restate deployment register …` invocation. Assumes the
    /// caller has already run `restate -y cloud login` and
    /// `restate -y cloud env config --env <your-env-name>` (or set
    /// `RESTATE_CLOUD_TOKEN`/`RESTATE_ENVIRONMENT` in CI).
    #[command(subcommand)]
    Restate(RestateCmd),
    /// Diagnose ongoing scheduled-job health: surface trigger Jobs wedged in
    /// `ImagePullBackOff`/`CrashLoopBackOff` (which, under a `CronJob`'s
    /// `concurrencyPolicy: Forbid`, silently skip every subsequent run) and
    /// workloads that aren't fully ready, each with the command that fixes it.
    /// Read-only `kubectl get` against the current context.
    Doctor {
        /// Namespace to inspect. Defaults to `NAVIGATOR_K8S_NAMESPACE` / `navigator`.
        #[arg(long)]
        namespace: Option<String>,
    },
    /// One-shot "ship to prod" — the executable path documented in
    /// `docs/cloud-operations.md`. CI (`deploy.yml`) builds and publishes
    /// the images to ghcr.io tagged `YY.MM.DD`; power-push only rolls the
    /// cluster. Default flow: resolve the `YY.MM.DD` ghcr tag (latest
    /// published, or `--tag`) → confirm the prod Secret satisfies the new
    /// binary's boot invariants → roll out BOTH deployments at that tag →
    /// pin every trigger `CronJob` to the same tag → re-register the worker
    /// with Restate, so every navigator image ends in sync at one
    /// `YY.MM.DD`. Reads every project / region / domain / cluster value
    /// from `.env`; never builds images locally.
    PowerPush {
        /// Print every command instead of running it.
        #[arg(long)]
        dry_run: bool,
        /// No-rebuild path: `kubectl rollout restart` BOTH deployments
        /// so the pods re-read a rotated Secret value, then exit. Use
        /// after rotating a key in the K8s Secret.
        #[arg(long)]
        restart_only: bool,
        /// The `YY.MM.DD` ghcr tag to roll onto. Omit to roll the latest
        /// published tag (resolved from ghcr). Both deployments are
        /// pinned to the same tag — never a version skew.
        #[arg(long)]
        tag: Option<String>,
    },
    /// DNS provisioning. Ensures MX / SPF / DMARC (+ optional DKIM)
    /// for a domain via the configured DNS provider (`DNSimple` today).
    /// Reads `DNSIMPLE_API_TOKEN` + `DNSIMPLE_ACCOUNT_ID` from the
    /// environment. Idempotent: existing matching records are no-ops.
    #[command(subcommand)]
    Dns(DnsCmd),
    /// White-label brand pack. `rebrand verify` validates `navigator.yaml`;
    /// `rebrand apply` compiles it into `NAVIGATOR_*` env vars and copies
    /// the logos into the public asset dir. See `navigator.example.yaml`.
    #[command(subcommand)]
    Rebrand(BrandCmd),
    /// Stand up the `OTel` Collector seam in prod and wire the binaries to
    /// it: ensure the `navigator-otel` GSA + telemetry-write IAM +
    /// Workload Identity, apply the Collector + self-monitoring
    /// manifests, and `envFrom` the shared `navigator-otel-env` `ConfigMap`
    /// onto `navigator-web` + `workflows-service` so
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` reaches `telemetry::init`. Idempotent;
    /// reads project/region/cluster/context from the environment. Run once
    /// per cluster, then `power-push` (or rollout-restart) the binaries.
    Observability {
        /// Print every command instead of running it.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum AssetsAction {
    /// Resize + re-encode every manifest photo into AVIF + WebP + JPEG
    /// width variants under `<out>/img/<slug>/`. Run after editing the
    /// `views::assets::GALLERY` manifest or replacing a source photo;
    /// variant paths are stable, so a bounded cache TTL serves the new
    /// bytes once the old ones expire (no cache-bust token).
    Build {
        /// Directory holding the source photos, named by each
        /// manifest entry's `source` field.
        #[arg(long, default_value = "/tmp/nav-photo-work/assets_src/jpeg")]
        src: PathBuf,
        /// Output root; variants land under `<out>/img/<slug>/`.
        /// Defaults to the crate-bundled `/public` mount so a local
        /// dev loop / `cargo test` serves the variants from `/public`.
        #[arg(long, default_value = "web/public")]
        out: PathBuf,
    },
    /// Push the built variant tree to the public assets bucket via the
    /// `cloud` crate's `StorageService` (never the GCP SDK directly).
    /// Each file lands under key `img/<slug>/<slug>-<w>w.<ext>` with a
    /// bounded `Cache-Control` (~1 week, no `immutable`). Run after
    /// `cli assets build`. Auth is ADC; the emulator endpoint is honored
    /// via `NAVIGATOR_STORAGE_ENDPOINT`.
    Upload {
        /// Directory holding the built variant tree.
        #[arg(long, default_value = "web/public/img")]
        dir: PathBuf,
        /// Target bucket. Defaults to `NAVIGATOR_ASSETS_BUCKET` — the
        /// public `<project>-assets` bucket, deliberately distinct from
        /// the app's documents bucket (`NAVIGATOR_DOCUMENTS_BUCKET`) so an
        /// upload never writes photos into the documents lane.
        #[arg(long, env = "NAVIGATOR_ASSETS_BUCKET")]
        bucket: Option<String>,
    },
    /// Restore the gitignored `web/public/img/` tree from the public
    /// assets bucket — the inverse of `upload`, for local development.
    /// A fresh clone has empty photo slots (`web/public/img/` is in
    /// `.gitignore`); this downloads every variant under the bucket's
    /// `img/` prefix so the `/public` mount serves the photos again,
    /// without the original source JPEGs. Read-only against the bucket;
    /// auth is ADC, the emulator endpoint is honored via
    /// `NAVIGATOR_STORAGE_ENDPOINT`.
    Pull {
        /// Output root; variants land under `<out>/<slug>/<file>` (the
        /// bucket's `img/` prefix is stripped). Defaults to the `/public`
        /// mount so a local dev loop serves them immediately.
        #[arg(long, default_value = "web/public/img")]
        out: PathBuf,
        /// Source bucket. Defaults to `NAVIGATOR_ASSETS_BUCKET` — the
        /// public `<project>-assets` bucket.
        #[arg(long, env = "NAVIGATOR_ASSETS_BUCKET")]
        bucket: Option<String>,
    },
}

#[derive(Subcommand)]
enum FormsAction {
    /// Push every vendored blank (the `forms` registry bundled from
    /// `notation_templates/forms/`) to the assets bucket at its FORMS.toml
    /// `object_path`. Idempotent — existing keys are skipped, since a
    /// form revision is immutable (a refresh lands at a new path).
    /// Auth is ADC; the emulator endpoint is honored via
    /// `NAVIGATOR_STORAGE_ENDPOINT`.
    Sync {
        /// Target bucket. Defaults to `NAVIGATOR_ASSETS_BUCKET`.
        #[arg(long, env = "NAVIGATOR_ASSETS_BUCKET")]
        bucket: Option<String>,
    },
}

#[derive(Subcommand)]
enum LspAction {
    /// Push prebuilt `navigator-lsp` binaries to the public assets
    /// bucket at `lsp/<triple>/navigator-lsp`, the key the `/lsp`
    /// download buttons resolve through. `--dir` is the cross-build
    /// output root laid out as `<dir>/<triple>/navigator-lsp` (see
    /// `docs/lsp/zed.md`); a target whose binary is absent is skipped,
    /// not an error. Auth is ADC; the emulator endpoint is honored via
    /// `NAVIGATOR_STORAGE_ENDPOINT`.
    Publish {
        /// Directory holding the per-target binaries
        /// (`<dir>/<triple>/navigator-lsp`).
        #[arg(long, default_value = "target/lsp-dist")]
        dir: PathBuf,
        /// Target bucket. Defaults to `NAVIGATOR_ASSETS_BUCKET` — the
        /// public `<project>-assets` bucket, distinct from the documents
        /// lane so the open-source binary never lands among confidential
        /// client documents.
        #[arg(long, env = "NAVIGATOR_ASSETS_BUCKET")]
        bucket: Option<String>,
    },
}

#[derive(Subcommand)]
enum DriveAction {
    /// Run the OAuth installed-app flow against Google's consent
    /// screen. Reads the client config at
    /// `~/.config/navigator/oauth_client.json`, opens a one-shot
    /// loopback listener on `127.0.0.1:8888` (override with
    /// `NAVIGATOR_DRIVE_CALLBACK_PORT`), and persists the resulting
    /// refresh token to `~/.config/navigator/drive_token.json` with
    /// `0o600`. Re-run to rotate.
    Login,
    /// List Drive contents. With no flags lists every shared drive
    /// the authenticated identity can see. With `--drive <id>` lists
    /// the root of that drive. Add `--folder <id>` to list a sub-
    /// folder.
    Ls {
        /// Shared drive id (the `0…` value from `cli drive ls`).
        #[arg(long)]
        drive: Option<String>,
        /// Folder id. When omitted alongside `--drive`, lists the
        /// drive root (whose folder id equals the drive id).
        #[arg(long)]
        folder: Option<String>,
    },
}

/// Shared `--host` selector for the live-site commands: optional, since a
/// single stored login is used by default.
#[derive(clap::Args)]
struct HostOpt {
    /// Target host. Optional when exactly one host is logged in.
    #[arg(long)]
    host: Option<String>,
}

#[derive(Subcommand)]
enum ProjectsAction {
    /// List the live site's projects as a table (or `--json`), parsed
    /// from `GET /portal/projects.csv`.
    List {
        #[command(flatten)]
        host: HostOpt,
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum RetainerAction {
    /// Approve a notation parked at `staff_review` (`POST …/approve-send`).
    /// Renders + parks: the worker durably renders + persists the retainer
    /// PDF and the workflow waits at `document_open__retainer_pdf`. No
    /// envelope is sent — run `retainer send` for that.
    Approve {
        /// Notation UUID printed by `project open`.
        notation_id: uuid::Uuid,
        #[command(flatten)]
        host: HostOpt,
    },
    /// Dispatch the rendered document for signature (`POST …/send`). On
    /// prod this emits exactly one real envelope — a deliberate,
    /// authenticated human command. Refuses with a retry hint (HTTP 409)
    /// when the worker hasn't rendered the PDF yet.
    Send {
        /// Notation UUID printed by `project open`.
        notation_id: uuid::Uuid,
        #[command(flatten)]
        host: HostOpt,
    },
    /// Add / edit / list the per-matter custom clauses spliced into the
    /// retainer at `{{custom_clauses}}` — the same paragraphs the admin
    /// clause editor manages, before the `staff_review` approval.
    Clause {
        #[command(subcommand)]
        action: ClauseAction,
    },
}

#[derive(Subcommand)]
enum ClauseAction {
    /// List a notation's custom clauses (`GET …/clauses?format=json`).
    List {
        /// Notation UUID.
        notation_id: uuid::Uuid,
        #[command(flatten)]
        host: HostOpt,
        /// Emit the raw JSON body.
        #[arg(long)]
        json: bool,
    },
    /// Append a clause (`POST …/clauses`).
    Add {
        /// Notation UUID.
        notation_id: uuid::Uuid,
        /// The clause prose (markdown).
        #[arg(long)]
        body: String,
        #[command(flatten)]
        host: HostOpt,
    },
    /// Replace a clause's body (`POST …/clauses/:cid/edit`).
    Edit {
        /// Notation UUID.
        notation_id: uuid::Uuid,
        /// Clause UUID printed by `clause list`.
        clause_id: uuid::Uuid,
        /// The new clause prose (markdown).
        #[arg(long)]
        body: String,
        #[command(flatten)]
        host: HostOpt,
    },
}

#[derive(Subcommand)]
enum MatterAction {
    /// Open a questionnaire-driven matter (an `onboarding__*` formation)
    /// and leave its questionnaire ready to walk with `intake answer`
    /// (`POST /portal/admin/retainers/new`). This is distinct from
    /// `project open`, which opens a matter *and* sends a retainer in one
    /// action; `matter open` sends nothing — it parks at the first
    /// question for the terminal walk.
    Open {
        #[command(flatten)]
        host: HostOpt,
        /// Onboarding template code, e.g. `onboarding__nest` (Nevada LLC).
        #[arg(long)]
        template: String,
        /// Client email — the matter's bound client (signer).
        #[arg(long)]
        client_email: String,
    },
}

#[derive(Subcommand)]
enum SubscriptionAction {
    /// Open a recurring subscription (`POST /portal/admin/subscriptions`).
    /// Starts `pending` — not billed until the linked project's retainer is
    /// signed — unless `--active` is passed. A discount comes from a
    /// `--coupon` code OR the inline `--discount-*` flags, never both.
    Create {
        #[command(flatten)]
        host: HostOpt,
        /// Recurring product code, e.g. `nexus` ($2,222/mo) or `nautilus`.
        #[arg(long)]
        product: String,
        /// Billing contact display name (the Xero contact).
        #[arg(long)]
        contact_name: String,
        /// Billing contact email (the Xero contact match key).
        #[arg(long)]
        contact_email: String,
        /// Apply a reusable coupon code (overrides the `--discount-*` flags).
        #[arg(long)]
        coupon: Option<String>,
        /// Inline whole-percent discount off list (0–100).
        #[arg(long)]
        discount_percent: Option<i64>,
        /// Inline flat discount off list, in cents.
        #[arg(long)]
        discount_amount_cents: Option<i64>,
        /// Link the project whose signed retainer activates this
        /// subscription.
        #[arg(long = "project")]
        project_id: Option<uuid::Uuid>,
        /// Link the billed organisation.
        #[arg(long = "entity")]
        entity_id: Option<uuid::Uuid>,
        /// Link the billed individual.
        #[arg(long = "person")]
        person_id: Option<uuid::Uuid>,
        /// Bill immediately — start `active`, skipping the retainer gate
        /// (for an already-signed or standalone engagement).
        #[arg(long)]
        active: bool,
    },
    /// List the live site's subscriptions
    /// (`GET /portal/admin/subscriptions?format=json`).
    List {
        #[command(flatten)]
        host: HostOpt,
        /// Emit the raw JSON body instead of a table.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CouponAction {
    /// Mint a reusable discount coupon (`POST /portal/admin/coupons`). Set a
    /// `--percent` OR an `--amount-cents`, not both.
    Create {
        /// The redeemable code staff hand a client, e.g. `FRIEND99`.
        code: String,
        #[command(flatten)]
        host: HostOpt,
        /// Whole-percent discount off list (0–100).
        #[arg(long = "percent")]
        discount_percent: Option<i64>,
        /// Flat discount off list, in cents.
        #[arg(long = "amount-cents")]
        discount_amount_cents: Option<i64>,
        /// Restrict the coupon to one product code (default: any product).
        #[arg(long = "product")]
        product: Option<String>,
        /// Optional expiry date, `YYYY-MM-DD` (rejected on/after, UTC).
        #[arg(long)]
        expires: Option<String>,
        /// Optional cap on how many subscriptions may apply this coupon.
        #[arg(long = "max")]
        max_redemptions: Option<i64>,
    },
    /// List the live site's coupons (`GET /portal/admin/coupons?format=json`).
    List {
        #[command(flatten)]
        host: HostOpt,
        /// Emit the raw JSON body instead of a table.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum IntakeAction {
    /// Walk a notation's questionnaire one question at a time over the
    /// same `/portal/admin/notations/:id/step` route the browser uses,
    /// reading each question's metadata from its `?format=json` branch.
    /// Interactive by default; pass `--answer` / `--person` to script it
    /// non-interactively (scalar answers consumed in order; people rows
    /// fed to the first `people_list` question).
    Answer {
        /// Notation UUID printed by `matter open`.
        notation_id: uuid::Uuid,
        #[command(flatten)]
        host: HostOpt,
        /// A scalar answer (string/date/radio). Repeatable; consumed in
        /// the order the questionnaire asks. Omit for an interactive walk.
        #[arg(long = "answer")]
        answers: Vec<String>,
        /// A `people_list` row as `name=…,street=…,city=…,state=…,zip=…`.
        /// Repeatable — one per person. Values may not contain a comma;
        /// use the interactive walk for addresses that do.
        #[arg(long = "person")]
        persons: Vec<String>,
    },
}

#[derive(Subcommand)]
enum NotationAction {
    /// Print a notation's workflow state + signature request id
    /// (`GET …/review?format=json`).
    Status {
        /// Notation UUID.
        notation_id: uuid::Uuid,
        #[command(flatten)]
        host: HostOpt,
        /// Emit the raw JSON status body.
        #[arg(long)]
        json: bool,
    },
    /// Render + park a notation's document for review (`POST
    /// …/approve-send`) — fills the bound packet (a formation's official
    /// Secretary-of-State form, or a retainer PDF). Idempotent once
    /// rendered.
    Approve {
        /// Notation UUID.
        notation_id: uuid::Uuid,
        #[command(flatten)]
        host: HostOpt,
    },
    /// Download a notation's rendered document (the filled packet) to a
    /// local file (`GET …/documents/document`).
    Document {
        /// Notation UUID.
        notation_id: uuid::Uuid,
        /// Path to write the PDF to.
        #[arg(long)]
        out: PathBuf,
        #[command(flatten)]
        host: HostOpt,
    },
}

#[derive(Subcommand)]
enum ProjectAction {
    /// Insert a new row in the `projects` table. By default runs
    /// migrate + seed first so the named `--entity-name` can
    /// resolve against the canonical seed. Pass
    /// `--skip-migrate-and-seed` when pointing at an
    /// already-managed Postgres (e.g. a production database) to
    /// avoid touching the schema or upserting seed rows.
    Create {
        /// Human-readable matter name, e.g. `"Shook Estate"`.
        #[arg(long)]
        name: String,
        /// Exact `entities.name` of the legal organization this
        /// Project tracks. Omit for a Project not yet bound to any
        /// Entity.
        #[arg(long)]
        entity_name: Option<String>,
        /// Email of the pre-existing **client** Person this matter is
        /// opened for — its client-side DRI. Required: every matter has a
        /// client of record, and it must be a `role = client` person
        /// (create the client first). The staff-side DRI defaults to the
        /// firm principal.
        #[arg(long)]
        client_email: String,
        /// Lifecycle status. `open`, `closed`, or `archived`.
        #[arg(long, default_value = "open")]
        status: String,
        /// Postgres connection URL. Falls back to the
        /// `DATABASE_URL` environment variable.
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        /// Skip migrate + seed — the caller owns the schema.
        /// Use this for an already-managed production Postgres
        /// where you don't want the canonical seed re-applied.
        #[arg(long)]
        skip_migrate_and_seed: bool,
    },
    /// Open a matter on a *live site* and send the retainer for
    /// e-signature in one action (`POST /portal/projects` over the stored
    /// bearer token). Parks at `staff_review`; the binding send happens
    /// only on `retainer send`. ("matter" stays the domain word in the
    /// templates and lifecycle prose; this is just the de-overloaded
    /// command name.)
    Open {
        #[command(flatten)]
        host: HostOpt,
        /// Matter name, e.g. `"Shook estate"`.
        #[arg(long)]
        name: String,
        /// Onboarding template code, e.g. `onboarding__retainer`.
        #[arg(long, default_value = "onboarding__retainer")]
        template: String,
        /// Client (signer) full name.
        #[arg(long)]
        client_name: String,
        /// Client (signer) email — the e-signature envelope's recipient.
        #[arg(long)]
        client_email: String,
        /// Scope-of-services line for the engagement (the inline
        /// `{{product_description}}` services line).
        #[arg(long, default_value = "")]
        scope: String,
        /// The matter's scope narrative ("this project's story").
        /// Persisted to `projects.description` and seeded as the
        /// retainer's first custom clause — an attorney-editable draft.
        #[arg(long, default_value = "")]
        description: String,
    },
}

#[derive(Subcommand)]
enum GitAction {
    /// Mint a Personal Access Token for a person. The plaintext is
    /// printed once and never recoverable — only its hash is stored.
    Token {
        /// Exact `persons.email` the token authenticates as.
        #[arg(long)]
        person: String,
        /// Project UUID to scope the token to. Omit to scope it to
        /// every Project the person participates in.
        #[arg(long)]
        project: Option<uuid::Uuid>,
        /// `read` (clone/fetch) or `write` (push).
        #[arg(long, default_value = "read")]
        scope: String,
        /// Lifetime in hours before the token expires.
        #[arg(long, default_value_t = 168)]
        ttl_hours: i64,
        /// Postgres connection URL. Falls back to `DATABASE_URL`.
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
    /// Print a Project's clone URL: `<base>/projects/<id>.git`.
    Url {
        /// Project UUID.
        #[arg(long)]
        project: uuid::Uuid,
        /// Public origin of the deployment, e.g.
        /// `https://www.your-domain.example`. Falls back to
        /// `NAVIGATOR_PUBLIC_BASE_URL`.
        #[arg(long, env = "NAVIGATOR_PUBLIC_BASE_URL")]
        base: String,
    },
}

#[derive(Subcommand)]
enum ListSubject {
    /// List every row in the `questions` table.
    Questions,
    /// List every row in the `templates` table.
    Templates,
    /// List every row in the `jurisdictions` table.
    Jurisdictions,
    /// List every row in the `persons` table.
    Persons,
    /// List every row in the `entities` table.
    Entities,
    /// List every row in the `entity_types` table.
    EntityTypes,
    /// List every row in the `projects` table.
    Projects,
    /// List every row in the `letters` table.
    Letters,
}

#[allow(clippy::too_many_lines)] // one flat dispatch match; splitting it hurts readability
fn main() -> ExitCode {
    // `.env` is picked up before `clap` reads its `env = "..."`
    // defaults. No-op when no file is present, so CI/cluster deploys
    // that inject env vars another way continue to work. The
    // `.devx/env` overlay carries values `devx up` derives at port-
    // forward time; `from_path` skips keys already set, so `.env`
    // wins.
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(".devx/env");
    let runtime = || tokio::runtime::Runtime::new().expect("tokio runtime");
    match Cli::parse().command {
        Command::Validate {
            dir,
            markdown_only,
            no_default_excludes,
            fix,
            database_url,
        } => runtime().block_on(run_validate(
            &dir,
            markdown_only,
            no_default_excludes,
            fix,
            database_url.as_deref(),
        )),
        Command::ValidateEvents { dir } => events::run_validate(&dir),
        Command::Render {
            file,
            out,
            format,
            answers,
        } => run_render(&file, &out, format.as_deref(), &answers),
        Command::Import { dir, database_url } => {
            runtime().block_on(run_import(&dir, &database_url))
        }
        Command::ImportContacts {
            file,
            dry_run,
            database_url,
        } => runtime().block_on(run_import_contacts(&file, dry_run, &database_url)),
        Command::List {
            subject,
            database_url,
        } => runtime().block_on(run_list(subject, &database_url)),
        Command::Erd {
            database_url,
            format,
        } => runtime().block_on(run_erd(&database_url, format)),
        Command::Assets { action } => match action {
            AssetsAction::Build { src, out } => assets::run_build(&src, &out),
            AssetsAction::Upload { dir, bucket } => assets::run_upload(&dir, bucket),
            AssetsAction::Pull { out, bucket } => assets::run_pull(&out, bucket),
        },
        Command::Forms { action } => match action {
            FormsAction::Sync { bucket } => forms_sync::run_sync(bucket),
        },
        Command::Lsp { action } => match action {
            LspAction::Publish { dir, bucket } => lsp_publish::run_publish(&dir, bucket),
        },
        Command::Project { action } => runtime().block_on(run_project(action)),
        Command::Git { action } => runtime().block_on(run_git_cmd(action)),
        Command::Drive { action } => runtime().block_on(run_drive(action)),
        Command::Login { host } => runtime().block_on(login::run_login(&host)),
        Command::Logout { host } => login::run_logout(host.as_deref()),
        Command::Whoami { host } => login::run_whoami(host.as_deref()),
        Command::Projects { action } => runtime().block_on(run_projects(action)),
        Command::Retainer { action } => runtime().block_on(run_retainer(action)),
        Command::Matter { action } => runtime().block_on(run_matter(action)),
        Command::Subscription { action } => runtime().block_on(run_subscription(action)),
        Command::Coupon { action } => runtime().block_on(run_coupon(action)),
        Command::Intake { action } => runtime().block_on(run_intake(action)),
        Command::Notation { action } => runtime().block_on(run_notation(action)),
        Command::Format { file } => format::run(&file),
        Command::Glossary { term } => glossary::run(term.as_deref()),
        Command::Scaffold {
            matter,
            category,
            jurisdiction,
        } => scaffold::run(
            &scaffold::workspace_root_from_cli_dir(),
            &matter,
            &category,
            &jurisdiction,
        ),
        Command::Transcribe {
            template,
            transcript,
            audio,
            speech_backend,
            google_project,
            google_location,
            google_language,
            google_model,
            pretty,
        } => runtime().block_on(run_transcribe(transcribe::CoverArgs {
            template,
            transcript,
            audio,
            speech_backend,
            google_project,
            google_location,
            google_language,
            google_model,
            pretty,
        })),
        // Local-development + deploy orchestration (collapsed in from the
        // former `devx` binary). The whole group routes through one handler.
        c @ (Command::StartDevServer
        | Command::Down
        | Command::Env
        | Command::Status
        | Command::KindUp
        | Command::KindDown
        | Command::Image
        | Command::ImageWorkflowsService
        | Command::ImageArchivesTrigger
        | Command::ImageStatutesTrigger
        | Command::ImageBillingCanaryTrigger
        | Command::ImageReconcileInvoicesTrigger
        | Command::ImageRecurringBillingTrigger
        | Command::ImageHeartbeatTrigger
        | Command::ImageBillingDigestTrigger
        | Command::Deploy
        | Command::Undeploy
        | Command::E2e
        | Command::GrantStaff
        | Command::Logs
        | Command::KustomizeKind
        | Command::KustomizeGke
        | Command::Gcp(_)
        | Command::Restate(_)
        | Command::Doctor { .. }
        | Command::PowerPush { .. }
        | Command::Dns(_)
        | Command::Rebrand(_)
        | Command::Observability { .. }) => devx_result(devx::dispatch(c)),
    }
}

/// Map an orchestration command's `anyhow::Result<()>` onto a process
/// `ExitCode`. The former `devx` binary printed the error chain and exited
/// non-zero; keep that behavior now that it runs under `navigator`.
fn devx_result(result: anyhow::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err:?}");
            ExitCode::FAILURE
        }
    }
}

async fn run_transcribe(args: transcribe::CoverArgs) -> ExitCode {
    match transcribe::cover(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("navigator: transcribe: {e:?}");
            ExitCode::from(2)
        }
    }
}

async fn open_postgres(database_url: &str) -> Result<sea_orm::DatabaseConnection, ExitCode> {
    sea_orm::Database::connect(database_url).await.map_err(|e| {
        eprintln!("navigator: open database `{database_url}`: {e}");
        ExitCode::from(2)
    })
}

async fn run_erd(database_url: &str, format: erd::OutputFormat) -> ExitCode {
    let db = match open_postgres(database_url).await {
        Ok(d) => d,
        Err(code) => return code,
    };
    if let Err(e) = store::migrate(&db).await {
        eprintln!("navigator: migrate: {e}");
        return ExitCode::from(2);
    }
    if let Err(e) = erd::run(&db, format).await {
        eprintln!("navigator: erd: {e}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

async fn run_list(subject: ListSubject, database_url: &str) -> ExitCode {
    let db = match open_postgres(database_url).await {
        Ok(d) => d,
        Err(code) => return code,
    };
    if let Err(e) = store::migrate(&db).await {
        eprintln!("navigator: migrate: {e}");
        return ExitCode::from(2);
    }
    let storage = match cloud::from_env().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("navigator: storage: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = store::seed::seed_canonical(&db, &storage).await {
        eprintln!("navigator: seed: {e}");
        return ExitCode::from(2);
    }
    let result = match subject {
        ListSubject::Questions => list::list_questions(&db).await,
        ListSubject::Templates => list::list_templates(&db).await,
        ListSubject::Jurisdictions => list::list_jurisdictions(&db).await,
        ListSubject::Persons => list::list_persons(&db).await,
        ListSubject::Entities => list::list_entities(&db).await,
        ListSubject::EntityTypes => list::list_entity_types(&db).await,
        ListSubject::Projects => list::list_projects(&db).await,
        ListSubject::Letters => list::list_letters(&db).await,
    };
    if let Err(e) = result {
        eprintln!("navigator: list: {e}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

async fn run_git_cmd(action: GitAction) -> ExitCode {
    match action {
        GitAction::Url { project, base } => {
            println!("{}", git::clone_url(&base, project));
            ExitCode::SUCCESS
        }
        GitAction::Token {
            person,
            project,
            scope,
            ttl_hours,
            database_url,
        } => {
            let db = match open_postgres(&database_url).await {
                Ok(conn) => conn,
                Err(code) => return code,
            };
            match git::mint_token(&db, &person, project, &scope, ttl_hours).await {
                Ok((plaintext, model)) => {
                    println!(
                        "{} {} ({} scope, expires {})",
                        palette::dim(format!("minted git token {}", model.id)),
                        palette::highlight(&person),
                        model.scope,
                        model.expires_at,
                    );
                    println!(
                        "{}",
                        palette::dim("paste this as the git password (shown once):")
                    );
                    println!("{plaintext}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("navigator: git token: {e}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

async fn run_project(action: ProjectAction) -> ExitCode {
    match action {
        ProjectAction::Create {
            name,
            entity_name,
            client_email,
            status,
            database_url,
            skip_migrate_and_seed,
        } => {
            run_project_create(
                &name,
                entity_name.as_deref(),
                &client_email,
                &status,
                &database_url,
                skip_migrate_and_seed,
            )
            .await
        }
        ProjectAction::Open {
            host,
            name,
            template,
            client_name,
            client_email,
            scope,
            description,
        } => {
            remote::matter_open(
                host.host.as_deref(),
                &remote::MatterOpen {
                    name,
                    template,
                    client_name,
                    client_email,
                    scope,
                    description,
                },
            )
            .await
        }
    }
}

async fn run_project_create(
    name: &str,
    entity_name: Option<&str>,
    client_email: &str,
    status: &str,
    database_url: &str,
    skip_migrate_and_seed: bool,
) -> ExitCode {
    let db = match open_postgres(database_url).await {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    if !skip_migrate_and_seed {
        if let Err(e) = store::migrate(&db).await {
            eprintln!("navigator: migrate: {e}");
            return ExitCode::from(2);
        }
        let storage = match cloud::from_env().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("navigator: storage: {e}");
                return ExitCode::from(2);
            }
        };
        if let Err(e) = store::seed::seed_canonical(&db, &storage).await {
            eprintln!("navigator: seed: {e}");
            return ExitCode::from(2);
        }
    }
    match project::create(&db, name, entity_name, client_email, status).await {
        Ok(p) => {
            println!(
                "{} {} (status={}, entity_id={})",
                palette::dim(format!("created project {}", p.id)),
                palette::highlight(&p.name),
                p.status,
                p.entity_id,
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("navigator: project create: {e}");
            ExitCode::from(2)
        }
    }
}

async fn run_drive(action: DriveAction) -> ExitCode {
    match action {
        DriveAction::Login => drive::run_login().await,
        DriveAction::Ls { drive, folder } => {
            drive::run_ls(drive.as_deref(), folder.as_deref()).await
        }
    }
}

async fn run_projects(action: ProjectsAction) -> ExitCode {
    match action {
        ProjectsAction::List { host, json } => {
            remote::projects_list(host.host.as_deref(), json).await
        }
    }
}

async fn run_retainer(action: RetainerAction) -> ExitCode {
    match action {
        RetainerAction::Approve { notation_id, host } => {
            remote::retainer_approve(host.host.as_deref(), notation_id).await
        }
        RetainerAction::Send { notation_id, host } => {
            remote::retainer_send(host.host.as_deref(), notation_id).await
        }
        RetainerAction::Clause { action } => run_clause(action).await,
    }
}

async fn run_clause(action: ClauseAction) -> ExitCode {
    match action {
        ClauseAction::List {
            notation_id,
            host,
            json,
        } => remote::clause_list(host.host.as_deref(), notation_id, json).await,
        ClauseAction::Add {
            notation_id,
            body,
            host,
        } => remote::clause_add(host.host.as_deref(), notation_id, &body).await,
        ClauseAction::Edit {
            notation_id,
            clause_id,
            body,
            host,
        } => remote::clause_edit(host.host.as_deref(), notation_id, clause_id, &body).await,
    }
}

async fn run_matter(action: MatterAction) -> ExitCode {
    match action {
        MatterAction::Open {
            host,
            template,
            client_email,
        } => remote::matter_walk_open(host.host.as_deref(), &template, &client_email).await,
    }
}

async fn run_subscription(action: SubscriptionAction) -> ExitCode {
    match action {
        SubscriptionAction::Create {
            host,
            product,
            contact_name,
            contact_email,
            coupon,
            discount_percent,
            discount_amount_cents,
            project_id,
            entity_id,
            person_id,
            active,
        } => {
            remote::subscription_create(
                host.host.as_deref(),
                &remote::SubscriptionCreate {
                    product,
                    contact_name,
                    contact_email,
                    coupon,
                    discount_percent,
                    discount_amount_cents,
                    project_id,
                    entity_id,
                    person_id,
                    active,
                },
            )
            .await
        }
        SubscriptionAction::List { host, json } => {
            remote::subscriptions_list(host.host.as_deref(), json).await
        }
    }
}

async fn run_coupon(action: CouponAction) -> ExitCode {
    match action {
        CouponAction::Create {
            code,
            host,
            discount_percent,
            discount_amount_cents,
            product,
            expires,
            max_redemptions,
        } => {
            remote::coupon_create(
                host.host.as_deref(),
                &remote::CouponCreate {
                    code,
                    discount_percent,
                    discount_amount_cents,
                    product,
                    expires,
                    max_redemptions,
                },
            )
            .await
        }
        CouponAction::List { host, json } => remote::coupons_list(host.host.as_deref(), json).await,
    }
}

async fn run_intake(action: IntakeAction) -> ExitCode {
    match action {
        IntakeAction::Answer {
            notation_id,
            host,
            answers,
            persons,
        } => remote::intake_answer(host.host.as_deref(), notation_id, answers, persons).await,
    }
}

async fn run_notation(action: NotationAction) -> ExitCode {
    match action {
        NotationAction::Status {
            notation_id,
            host,
            json,
        } => remote::notation_status(host.host.as_deref(), notation_id, json).await,
        NotationAction::Approve { notation_id, host } => {
            remote::notation_approve(host.host.as_deref(), notation_id).await
        }
        NotationAction::Document {
            notation_id,
            out,
            host,
        } => remote::notation_document(host.host.as_deref(), notation_id, &out).await,
    }
}

/// The `N111` cross-file code-uniqueness pass for `validate`. Builds its
/// own filter because the lint engine takes ownership of the primary one.
fn code_uniqueness_pass(
    dir: &std::path::Path,
    no_default_excludes: bool,
) -> std::io::Result<Vec<rules::Violation>> {
    let filter: Box<dyn rules::FileFilter> = if no_default_excludes {
        Box::new(rules::DefaultFileFilter::without_default_excludes())
    } else {
        Box::new(rules::DefaultFileFilter::default())
    };
    rules::code_uniqueness_violations(dir, filter.as_ref())
}

async fn run_validate(
    dir: &std::path::Path,
    markdown_only: bool,
    no_default_excludes: bool,
    fix: bool,
    database_url: Option<&str>,
) -> ExitCode {
    let question_codes = if markdown_only {
        Vec::new()
    } else if let Some(url) = database_url {
        let db = match open_postgres(url).await {
            Ok(d) => d,
            Err(code) => return code,
        };
        match import::load_question_codes(&db).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("navigator: load question codes: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        Vec::new()
    };
    let filter: Box<dyn rules::FileFilter> = if no_default_excludes {
        Box::new(rules::DefaultFileFilter::without_default_excludes())
    } else {
        Box::new(rules::DefaultFileFilter::default())
    };
    if fix {
        let fix_report = match fix_directory(dir, filter.as_ref(), |file| {
            if markdown_only {
                rules::navigator_markdown_only_rules()
            } else {
                rules::navigator_classified_rules_with_codes(file, &question_codes)
            }
        }) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("navigator: {e}");
                return ExitCode::from(2);
            }
        };
        for path in &fix_report.fixed_files {
            println!("{}", palette::dim(format!("fixed {}", path.display())));
        }
        for v in &fix_report.remaining {
            print_violation(&v.path.display().to_string(), v.line, v.code, &v.message);
        }
        println!(
            "{}",
            palette::dim(format!(
                "Fixed {} file(s); {} remaining violation(s) need a human.",
                fix_report.fixed_files.len(),
                fix_report.remaining.len(),
            ))
        );
        return if fix_report.remaining.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        };
    }
    let report = if markdown_only {
        let engine =
            rules::RuleEngine::new(rules::navigator_markdown_only_rules()).with_filter(filter);
        engine.lint_directory(dir)
    } else {
        let engine = rules::ClassifiedRuleEngine::new()
            .with_question_codes(question_codes)
            .with_filter(filter);
        engine.lint_directory(dir)
    };
    let mut report = match report {
        Ok(r) => r,
        Err(e) => {
            eprintln!("navigator: {e}");
            return ExitCode::from(2);
        }
    };
    // Cross-file `N111`: notation template `code` must be unique across
    // the tree. Only meaningful for the classified (notation) rule set,
    // not the markdown-only prose pass.
    if !markdown_only {
        match code_uniqueness_pass(dir, no_default_excludes) {
            Ok(mut v) => report.violations.append(&mut v),
            Err(e) => {
                eprintln!("navigator: {e}");
                return ExitCode::from(2);
            }
        }
    }
    for v in &report.violations {
        print_violation(&v.path.display().to_string(), v.line, v.code, &v.message);
    }
    println!(
        "{}",
        palette::dim(format!(
            "Scanned {} file(s), found {} violation(s)",
            report.files_scanned,
            report.violations.len()
        ))
    );
    // Fail the gate on Error-severity violations only; Warning-severity
    // advisories (e.g. a step that's allowed but not built yet) are
    // printed above but do not make `validate` exit nonzero.
    if report.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Parse a `--answer code=value` argument into its halves. The value
/// may itself contain `=`; only the first `=` splits.
fn parse_answer(raw: &str) -> Result<(String, String), String> {
    let (code, value) = raw
        .split_once('=')
        .ok_or_else(|| format!("expected `code=value`, got `{raw}`"))?;
    if code.is_empty() {
        return Err(format!("empty answer code in `{raw}`"));
    }
    Ok((code.to_string(), value.to_string()))
}

/// Render one notation template to a PDF. Validates the file against the
/// notation rule set, resolves the output format (CLI override →
/// `output:` frontmatter → plain), fills any `{{code}}` placeholders
/// from `answers`, and writes the compiled PDF to `out`.
fn run_render(
    file: &std::path::Path,
    out: &std::path::Path,
    format_override: Option<&str>,
    answers: &[(String, String)],
) -> ExitCode {
    let contents = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("navigator: read {}: {e}", file.display());
            return ExitCode::from(2);
        }
    };

    // Gate on validation: only a clean notation template renders. Use
    // the same DB-free classified rule set as `validate`.
    let source = rules::SourceFile {
        path: file.to_path_buf(),
        contents: contents.clone(),
    };
    let violations: Vec<rules::Violation> =
        rules::navigator_classified_rules_with_codes(&source, &[])
            .iter()
            .flat_map(|r| r.lint(&source))
            .collect();
    if !violations.is_empty() {
        for v in &violations {
            print_violation(&v.path.display().to_string(), v.line, v.code, &v.message);
        }
        eprintln!(
            "navigator: {} validation violation(s); not rendering",
            violations.len()
        );
        return ExitCode::from(1);
    }

    // Resolve the output format: explicit flag wins, else the
    // template's `output:` field, else plain.
    let declared = rules::frontmatter::extract(&contents)
        .and_then(|fm| rules::frontmatter::field(fm, "output"))
        .filter(|s| !s.is_empty());
    let format_name = format_override.map(str::to_string).or(declared);
    let format = match format_name.as_deref().map(pdf::OutputFormat::parse) {
        // No format declared anywhere: render a plain document.
        None => pdf::OutputFormat::Plain,
        Some(Some(f)) => f,
        Some(None) => {
            let name = format_name.unwrap_or_default();
            // Derive the accepted list from the format enum so a new
            // variant shows up in the hint without a manual edit here.
            // `plain` is the implicit default and absent from
            // `FRONTMATTER_VALUES`, so prepend it.
            let known = std::iter::once("plain")
                .chain(pdf::OutputFormat::FRONTMATTER_VALUES.iter().copied())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!("navigator: unknown --format `{name}` (expected one of: {known})");
            return ExitCode::from(2);
        }
    };

    // Body is everything after the frontmatter block; fill placeholders.
    let mut body = strip_frontmatter(&contents).to_string();
    for (code, value) in answers {
        body = body.replace(&format!("{{{{{code}}}}}"), value);
    }

    // Source the letterhead from the canonical firm brand so the
    // rendered address honors the same `NAVIGATOR_*` overrides as the
    // website footer.
    let brand = &views::brand::FIRM_BRAND;
    let letterhead = pdf::Letterhead {
        name: brand.site_name.to_string(),
        contact: "neonlaw.com".to_string(),
        address: brand.postal_address.to_string(),
    };
    let bytes = match pdf::render_document(&body, format, &letterhead) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("navigator: render {}: {e}", file.display());
            return ExitCode::from(2);
        }
    };
    if let Err(e) = std::fs::write(out, &bytes) {
        eprintln!("navigator: write {}: {e}", out.display());
        return ExitCode::from(2);
    }
    println!(
        "{}",
        palette::dim(format!(
            "Rendered {} ({format:?}, {} bytes) → {}",
            file.display(),
            bytes.len(),
            out.display()
        ))
    );
    ExitCode::SUCCESS
}

/// Return the body of a notation file — everything after the leading
/// YAML frontmatter block. When there is no recognized frontmatter, the
/// whole string is the body.
fn strip_frontmatter(contents: &str) -> &str {
    let Some(after_open) = contents.strip_prefix("---\n") else {
        return contents;
    };
    if let Some(end) = after_open.find("\n---\n") {
        return after_open[end + "\n---\n".len()..].trim_start_matches('\n');
    }
    // Closer at EOF with no body, or no closer at all.
    after_open.strip_suffix("\n---").map_or(contents, |_| "")
}

struct FixReport {
    fixed_files: Vec<PathBuf>,
    remaining: Vec<rules::Violation>,
}

/// Walk `dir` honoring `filter`, apply every safe-by-construction
/// autofix to each markdown file in place, and then re-lint to
/// collect the diagnostic-only violations a human still needs to
/// address. Edits within a file are applied highest-offset-first so
/// earlier offsets stay valid; on overlap the rule with the lower
/// code string wins (deterministic).
fn fix_directory(
    dir: &std::path::Path,
    filter: &dyn rules::FileFilter,
    rules_for_file: impl Fn(&rules::SourceFile) -> Vec<Box<dyn rules::Rule>>,
) -> std::io::Result<FixReport> {
    let mut fixed_files = Vec::new();
    let mut remaining = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() && e.depth() > 0 {
                filter.include_dir(e.path())
            } else {
                true
            }
        })
    {
        let entry = entry.map_err(std::io::Error::other)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !filter.include_file(path) {
            continue;
        }
        let contents = std::fs::read_to_string(path)?;
        let mut file = rules::SourceFile {
            path: path.to_path_buf(),
            contents,
        };
        let rule_set = rules_for_file(&file);
        let mut edits: Vec<(rules::TextEdit, &'static str)> = Vec::new();
        for rule in &rule_set {
            for v in rule.lint(&file) {
                if let Some(edit) = rule.fix(&file, &v) {
                    edits.push((edit, rule.code()));
                }
            }
        }
        if !edits.is_empty() {
            // Sort ascending by start; resolve overlap by keeping the
            // lower-coded edit. Then apply descending.
            edits.sort_by(|a, b| a.0.range.start.cmp(&b.0.range.start).then(a.1.cmp(b.1)));
            let mut kept: Vec<(rules::TextEdit, &'static str)> = Vec::with_capacity(edits.len());
            for (edit, code) in edits {
                if let Some(prev) = kept.last() {
                    if edit.range.start < prev.0.range.end {
                        continue;
                    }
                }
                kept.push((edit, code));
            }
            kept.sort_by_key(|edit| std::cmp::Reverse(edit.0.range.start));
            let mut new_contents = file.contents.clone();
            for (edit, _) in &kept {
                new_contents.replace_range(edit.range.clone(), &edit.new_text);
            }
            if new_contents != file.contents {
                std::fs::write(path, &new_contents)?;
                fixed_files.push(path.to_path_buf());
                file.contents = new_contents;
            }
        }
        for rule in &rule_set {
            remaining.extend(rule.lint(&file));
        }
    }
    Ok(FixReport {
        fixed_files,
        remaining,
    })
}

/// Render a single rule violation: path/line in dim cyan-700, rule
/// code in cyan-500, message in default. Shared by validate and
/// import so both subcommands have the same look.
fn print_violation(path: &str, line: usize, code: &str, message: &str) {
    println!(
        "{} {}: {}",
        palette::dim(format!("{path}:{line}")),
        palette::highlight(code),
        message,
    );
}

async fn run_import(dir: &std::path::Path, database_url: &str) -> ExitCode {
    let db = match open_postgres(database_url).await {
        Ok(d) => d,
        Err(code) => return code,
    };
    if let Err(e) = store::migrate(&db).await {
        eprintln!("navigator: migrate: {e}");
        return ExitCode::from(2);
    }
    let storage = match cloud::from_env().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("navigator: storage: {e}");
            return ExitCode::from(2);
        }
    };
    let report = match import::import_directory(&db, &storage, dir).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("navigator: import: {e}");
            return ExitCode::from(2);
        }
    };
    for v in &report.violations {
        print_violation(&v.path.display().to_string(), v.line, v.code, &v.message);
    }
    println!(
        "{}",
        palette::dim(format!(
            "Imported {} template(s), {} question(s); skipped {} file(s) with rule violations.",
            report.templates_created,
            report.questions_created,
            report.files_skipped_due_to_violations,
        ))
    );
    if report.files_skipped_due_to_violations > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `cli import-contacts` — bulk-import organizations and people from a
/// JSON/YAML file. `::import::` (leading `::`) is the workspace crate,
/// distinct from this binary's own `mod import` (template importer).
async fn run_import_contacts(
    file: &std::path::Path,
    dry_run: bool,
    database_url: &str,
) -> ExitCode {
    let bytes = match std::fs::read_to_string(file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("navigator: read `{}`: {e}", file.display());
            return ExitCode::from(2);
        }
    };
    let payload = match ::import::parse(&bytes) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("navigator: parse `{}`: {e}", file.display());
            return ExitCode::from(2);
        }
    };

    // Dry run stops at structural validation — no database touched.
    if dry_run {
        let diagnostics = ::import::validate(&payload);
        print_import_diagnostics(&diagnostics);
        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == ::import::Severity::Error)
            .count();
        println!(
            "{}",
            palette::dim(format!(
                "Dry run: {} organization(s), {} person(s), {errors} error(s).",
                payload.organizations.len(),
                payload.people.len(),
            ))
        );
        return if errors > 0 {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        };
    }

    let db = match open_postgres(database_url).await {
        Ok(d) => d,
        Err(code) => return code,
    };
    if let Err(e) = store::migrate(&db).await {
        eprintln!("navigator: migrate: {e}");
        return ExitCode::from(2);
    }
    let report = match ::import::apply(&db, &payload).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("navigator: import-contacts: {e}");
            return ExitCode::from(2);
        }
    };

    print_import_diagnostics(&report.diagnostics);
    for row in report.organizations.iter().chain(&report.people) {
        if let Some(detail) = &row.detail {
            println!(
                "  {} {} — {}",
                palette::highlight(format!("{:?}", row.status)),
                row.key,
                detail,
            );
        }
    }
    println!("{}", palette::dim(report.summary()));
    if report.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn print_import_diagnostics(diagnostics: &[::import::Diagnostic]) {
    for d in diagnostics {
        let tag = match d.severity {
            ::import::Severity::Error => palette::highlight("error"),
            ::import::Severity::Warning => palette::dim("warn".to_string()),
        };
        println!("{tag} {}: {}", d.pointer, d.message);
    }
}
