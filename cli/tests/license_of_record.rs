//! Pin the licence of record: two organizations, two outbound grants, and no
//! way back.
//!
//! Root `LICENSE.md` governs the work. The software is dual-licensed
//! `MIT OR Apache-2.0` — the Rust ecosystem's default pair, permissive by MIT
//! and patent-granting by Apache-2.0 — and the drafted legal prose under
//! `templates/` is `CC-BY-4.0`, because attribution is the obligation that
//! actually fits a document somebody signs.
//!
//! An open-source licence is a promise to everyone who has cloned the
//! repository, and it cannot be quietly taken back: every copy keeps the rights
//! it was given, whatever a later commit says. The risk this file guards is
//! therefore an accidental *retraction*. A manifest drifting to
//! `LicenseRef-Proprietary`, an `EULA.md` appearing, or an "all rights
//! reserved" line landing in the licence file would each publish a
//! contradiction: the tree says one thing and the licence file another, and a
//! downstream reader has no way to tell which binds. The forbidden-strings
//! lists below name the exact clauses that would do it.
//!
//! The trademark reservation is guarded just as hard, and for a reason the
//! copyright grant does not cover. A permissive licence invites forks, and a
//! fork wearing the operating firm's name would misdirect the one person least
//! able to check who is accountable for their legal work. The marks are the
//! only thing this repository withholds, so a notice that goes missing or names
//! the wrong registrant is the failure that matters most.
//!
//! Structure only, never prose. The wording is expected to keep moving; only a
//! change to the *structure* — the owner changing, a manifest drifting off the
//! tag, a grant file disappearing, the reservation going missing — lands here.

mod common;
use common::is_sops_ciphertext;

use std::fs;
use std::path::{Path, PathBuf};

/// The SPDX expression every manifest in the workspace carries.
const LICENSE: &str = "MIT OR Apache-2.0";

/// The licence the drafted legal prose carries instead. A software licence is
/// written for software; a retainer or an intake questionnaire is a drafted
/// document, and the obligation worth enforcing on one is attribution.
const CONTENT_LICENSE: &str = "CC-BY-4.0";

/// The copyright holder: the organization that *produces* this software and
/// makes the outbound grant.
///
/// A rename edits this constant and root `LICENSE.md` together, and nothing
/// else in this file. It is the legal person rather than the trade name on
/// purpose: a copyright notice has to name someone who can hold a copyright,
/// and "Neon Law" alone is a brand.
const OWNER: &str = "Neon Law Foundation";

/// The trademark registrant, which is a *different* organization from the
/// copyright holder above.
///
/// The Foundation *produces* the software; the Firm *operates* it and owns the
/// mark, which the Foundation in turn uses under written permission.
/// Collapsing the two would be the easy mistake, and it is the one that
/// matters: a reader deciding whether they may ship a fork called "Neon Law"
/// needs to know the permission they would be asking for does not come from
/// whoever holds the copyright.
const REGISTRANT: &str = "Shook Law PLLC";

/// Grant files that must exist, because `LICENSE.md` points a reader at them.
/// A dual licence whose second half is a dead link is a single licence.
const GRANT_FILES: [&str; 2] = ["LICENSE-MIT", "LICENSE-APACHE"];

/// Licence files that must not exist, because each contradicts the grant.
///
/// `EULA.md` is the important one: it withholds the right to redistribute,
/// which both grants here permit, so its presence would leave two instruments
/// making opposite claims about the same bytes. `LICENSE-BUSL.txt` is the same
/// hazard from the other direction — a source-available grant with a four-year
/// clock over code published without one.
const RETIRED_LICENSE_FILES: [&str; 4] = [
    "EULA.md",
    "LICENSE-BUSL.txt",
    "FIAT_LICENSE.md",
    "NEON_LICENSE.md",
];

/// The workspace root (this test crate is `cli`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The prose surrounding a match — 200 characters either side, snapped out to
/// the nearest character boundary so a multi-byte dash in the copy cannot panic
/// the slice.
fn window(body: &str, at: usize, len: usize) -> &str {
    let mut start = at.saturating_sub(200);
    while start > 0 && !body.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (at + len + 200).min(body.len());
    while end < body.len() && !body.is_char_boundary(end) {
        end += 1;
    }
    &body[start..end]
}

/// Every `Cargo.toml` in the workspace, including manifests the workspace
/// excludes, found by walking rather than by a hand-listed set.
fn cargo_manifests() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !matches!(
                    name.as_ref(),
                    "target" | ".git" | "node_modules" | ".worktrees"
                ) {
                    walk(&path, out);
                }
            } else if name == "Cargo.toml" {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&repo_root(), &mut out);
    out.sort();
    out
}

/// Every crate either inherits the workspace license or declares the tag.
#[test]
fn every_crate_declares_or_inherits_the_license_of_record() {
    let manifests = cargo_manifests();
    assert!(
        manifests.len() > 20,
        "expected the whole workspace, found only {} manifests — the walk is \
         probably rooted wrong",
        manifests.len()
    );

    let mut offenders = Vec::new();
    for path in &manifests {
        let body =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let doc: toml::Value =
            toml::from_str(&body).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

        let package = doc
            .get("workspace")
            .and_then(|w| w.get("package"))
            .or_else(|| doc.get("package"));
        let Some(package) = package else {
            continue;
        };
        let Some(license) = package.get("license") else {
            offenders.push(format!("{}: no `license` field", path.display()));
            continue;
        };
        let inherits = license
            .get("workspace")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        if inherits {
            continue;
        }
        match license.as_str() {
            Some(LICENSE) => {}
            other => offenders.push(format!(
                "{}: license is {other:?}, expected {LICENSE:?} or \
                 `license.workspace = true`",
                path.display()
            )),
        }
    }

    assert!(
        offenders.is_empty(),
        "these manifests do not carry the workspace license of record:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn workspace_root_pins_the_license_of_record() {
    let doc: toml::Value = toml::from_str(&read("Cargo.toml")).expect("parse workspace Cargo.toml");
    let license = doc["workspace"]["package"]["license"]
        .as_str()
        .expect("workspace license is a string");
    assert_eq!(
        license, LICENSE,
        "the workspace license of record must stay `{LICENSE}`; every member \
         crate inherits this value"
    );
}

/// The VS Code extension ships outside the Cargo workspace (npm registry), so
/// its manifest declares the tag by hand and drifts on its own. It is now the
/// only manifest in the tree that can.
#[test]
fn editor_extension_manifest_declares_the_license_of_record() {
    let vscode: serde_json::Value =
        serde_json::from_str(&read("lsp/vscode-ext/package.json")).expect("parse package.json");
    assert_eq!(
        vscode["license"].as_str(),
        Some(LICENSE),
        "the VS Code extension manifest must declare `{LICENSE}`"
    );
}

/// Root `LICENSE.md` names the copyright holder and carries the SPDX tag.
#[test]
fn root_license_names_the_owner_and_the_spdx_tag() {
    let license = read("LICENSE.md");
    assert!(
        license.contains(&format!("Copyright (c) 2026 {OWNER}")),
        "root LICENSE.md must carry the copyright line `Copyright (c) 2026 {OWNER}`"
    );
    assert!(
        license.contains(&format!("SPDX-License-Identifier: {LICENSE}")),
        "root LICENSE.md must carry `SPDX-License-Identifier: {LICENSE}`"
    );
}

/// Both grants exist and are the real texts.
///
/// `LICENSE.md` offers the reader a choice between MIT and Apache-2.0, so both
/// texts have to be present and complete: a dual licence whose second half is a
/// dead link is a single licence, and the reader cannot tell which one they got.
///
/// Apache-2.0 is checked section by section rather than by length. Its patent
/// grant (§ 3) and redistribution conditions (§ 4) are the two clauses that make
/// choosing it different from choosing MIT, and a truncated paste that stopped
/// after the definitions would still look like a licence file.
#[test]
fn both_grant_texts_ship_and_are_complete() {
    for grant in GRANT_FILES {
        assert!(
            repo_root().join(grant).exists(),
            "{grant} is named by LICENSE.md and must exist — a dual licence \
             whose second half is missing is a single licence"
        );
    }

    let mit = read("LICENSE-MIT");
    for required in [
        "MIT License",
        &format!("Copyright (c) 2026 {OWNER}") as &str,
        "Permission is hereby granted, free of charge",
        "The above copyright notice and this permission notice shall be included",
    ] {
        assert!(
            mit.contains(required),
            "LICENSE-MIT must carry the verbatim MIT text; `{required}` is missing"
        );
    }

    let apache = read("LICENSE-APACHE");
    for required in [
        "Apache License",
        "Version 2.0, January 2004",
        "TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION",
        "1. Definitions.",
        "2. Grant of Copyright License.",
        "3. Grant of Patent License.",
        "4. Redistribution.",
        "6. Trademarks.",
        "END OF TERMS AND CONDITIONS",
    ] {
        assert!(
            apache.contains(required),
            "LICENSE-APACHE must carry the verbatim Apache-2.0 text; `{required}` \
             is missing, so the grant a reader may choose is incomplete"
        );
    }
}

/// The marks are reserved, and `LICENSE.md` says so in the same breath as the
/// grant.
///
/// This is the one reservation a permissive licence still needs here, and it is
/// the clause most likely to be lost in a rewrite, because every other sentence
/// in the file is about giving things away. Apache-2.0 § 6 already withholds
/// trademark rights, but a reader deciding whether they may ship a fork called
/// "Neon Law" reads `LICENSE.md`, not § 6 — and the answer has to be there.
#[test]
fn the_licence_reserves_the_marks_alongside_the_grant() {
    let flat = read("LICENSE.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(
        flat.contains("rights in copyright, not in trademarks"),
        "LICENSE.md must state that the grant covers copyright and not \
         trademarks — a permissive licence invites forks, and the marks are the \
         only thing this repository withholds from one"
    );
    assert!(
        flat.contains("6,325,650"),
        "LICENSE.md must cite the NEON LAW registration it reserves"
    );
    assert!(
        flat.contains("views::brand_bundle"),
        "LICENSE.md must point a fork at the brand manifest — telling someone \
         they may not use the marks without showing them the rename seam \
         leaves patching sources as the obvious move"
    );
}

/// The grant cannot be walked back.
///
/// This is the guard that matters most. The software is open source under two
/// permissive licences; a reader who clones it holds real rights, and every
/// copy already taken keeps them regardless of what a later commit says.
///
/// So the risk is an accidental *retraction*: a proprietary clause landing in
/// the licence file would leave the repository
/// claiming to be private while its own licence file grants the world a
/// licence. Both cannot be true, and the one people have already relied on is
/// the grant.
#[test]
fn the_license_grants_the_public_something_and_cannot_take_it_back() {
    // Flattened, because these are phrases and the Markdown line width splits
    // phrases: a raw `contains` would read a refilled paragraph as a deleted
    // clause.
    let license = read("LICENSE.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    for required in [
        "mit license",
        "apache license, version 2.0",
        "at your option",
        "open source",
    ] {
        assert!(
            license.contains(required),
            "root LICENSE.md must state `{required}` — the software is dual \
             licensed and the reader picks"
        );
    }

    // The retired vocabulary, matched as whole clauses rather than bare words.
    // "All Rights Reserved" cannot be forbidden on its own — a copyright line
    // may legitimately carry it — so the clauses named here are the ones the
    // proprietary drafts actually used.
    for retracted in [
        "licenseref-proprietary",
        "this is a private repository",
        "not source-available",
        "access to this repository is not a licence",
        "access is not a licence",
        "no licence is granted to anyone",
        "confidential, proprietary property",
        "may not be published, open-sourced, mirrored",
        // Source-available vocabulary. Any of it here would put a four-year
        // embargo over code published without one.
        "business source license",
        "additional use grant",
        "change date",
    ] {
        assert!(
            !license.contains(retracted),
            "root LICENSE.md must not retract or narrow the outbound grant; \
             found `{retracted}`. The software is published under {LICENSE} and \
             every copy already taken keeps its rights — a clause like this \
             makes the repository lie about what its readers already hold."
        );
    }
}

/// No retired licence file returns, and nothing still cites one.
///
/// A returning `EULA.md` would be the live contradiction: `LICENSE.md` grants
/// redistribution, and the EULA expressly withheld it over the same
/// executable.
#[test]
fn no_retired_license_file_returns() {
    assert!(
        repo_root().join("LICENSE.md").exists(),
        "LICENSE.md is the license of record and must exist"
    );
    for retired in RETIRED_LICENSE_FILES {
        assert!(
            !repo_root().join(retired).exists(),
            "{retired} is retired; {LICENSE} is the only instrument over this \
             Software and it already grants what that file withheld"
        );
    }
}

/// No manifest, workflow, or dependency policy still declares the retired
/// proprietary tag. A stale `LicenseRef-Proprietary` in a manifest tells a
/// downstream reader they hold no rights, when in fact they hold MIT's and
/// Apache-2.0's.
#[test]
fn no_surface_still_declares_the_retired_proprietary_tag() {
    let mut hits = Vec::new();
    for rel in [
        "Cargo.toml",
        "lsp/vscode-ext/package.json",
        "deny.toml",
        ".github/workflows/deploy.yml",
        "README.md",
        "CONTRIBUTING.md",
        "docs/licensing.md",
    ] {
        let path = repo_root().join(rel);
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
        for stale in [
            "LicenseRef-Proprietary",
            "EULA.md",
            "not source-available",
            "Access is not a licence",
            "access to this repository is not a licence",
            // A manifest on `BUSL-1.1` would tell a downstream reader their
            // rights expire on a clock that does not exist.
            "BUSL-1.1",
            "LICENSE-BUSL",
        ] {
            if flat.contains(stale) {
                hits.push(format!("{rel}: `{stale}`"));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "these surfaces still reference the retired proprietary licensing:\n  {}",
        hits.join("\n  ")
    );
}

/// Contributions are closed, contributions are inbound = outbound, and
/// `CONTRIBUTING.md` states both without letting either read as the other.
///
/// These are two independent facts and the file has to keep them apart. Closed
/// is a *capacity* decision about pull requests, revocable at will. Inbound =
/// outbound is the licensing position, and it is stated in advance so a fork's
/// authors know the terms without having to ask. A reader who conflates them
/// concludes that a closed door means the grant is closed too, which is the one
/// thing this repository can never say — every copy already taken keeps its
/// rights.
///
/// So the closed notice must arrive with a way to reach a human. A door with no
/// address behind it is what makes an open-source project look abandoned rather
/// than deliberate, and a security report has to land somewhere.
///
/// The "Contributor License and Feedback Agreement" must not appear: it names
/// an acceptance ledger and a `cla` gate this workspace does not have, and
/// there is nothing here for a contributor to sign.
#[test]
fn contributions_are_closed_but_the_licence_terms_are_stated_anyway() {
    const RETIRED_AGREEMENT: &str = "Contributor License and Feedback Agreement";

    /// Where someone turned away by the notice is told to write instead.
    const CONTACT: &str = "contact@neonlaw.org";

    let contributing = read("CONTRIBUTING.md");
    let flat = contributing
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    assert!(
        flat.contains("closed to outside contributions"),
        "CONTRIBUTING.md must say plainly that contributions are closed; a \
         contributor should learn that before opening a pull request, not after"
    );
    assert!(
        contributing.contains(CONTACT),
        "CONTRIBUTING.md must give `{CONTACT}` as the way to reach a human — a \
         closed door with no address behind it reads as an abandoned project, \
         and a security report still has to land somewhere"
    );

    assert!(
        contributing.contains(LICENSE),
        "CONTRIBUTING.md must name `{LICENSE}` as the terms a contribution is \
         licensed under; closed to pull requests is not closed to the grant"
    );
    assert!(
        flat.contains("inbound = outbound"),
        "CONTRIBUTING.md must keep stating the inbound = outbound position, so \
         the terms are knowable in advance and a fork's own authors know where \
         they stand"
    );
    assert!(
        contributing.contains(OWNER),
        "CONTRIBUTING.md must name `{OWNER}`"
    );
    for rel in ["CONTRIBUTING.md", "LICENSE.md", "docs/licensing.md"] {
        assert!(
            !read(rel).contains(RETIRED_AGREEMENT),
            "{rel} still cites the retired `{RETIRED_AGREEMENT}`; contributions \
             are inbound = outbound and there is nothing to sign"
        );
    }
}

#[test]
fn readme_states_the_license_of_record() {
    let readme = read("README.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        readme.contains(OWNER),
        "README.md must name `{OWNER}` as the copyright holder of Neon Law Navigator"
    );
    assert!(
        readme.contains(LICENSE),
        "README.md must name `{LICENSE}` as the software's licence"
    );
    assert!(
        readme.contains("CC BY 4.0") || readme.contains(CONTENT_LICENSE),
        "README.md must name the content licence the `templates/` prose carries"
    );
    for retired in RETIRED_LICENSE_FILES {
        assert!(
            !readme.contains(retired),
            "README.md must not link the retired licence file `{retired}`"
        );
    }
    assert!(
        !readme.contains("AGPL") && !readme.contains("Affero"),
        "README.md must not claim Affero/AGPL terms"
    );
}

/// The drafted legal prose is licensed apart from the software, and freer.
///
/// `templates/` holds documents a client signs, not programs. A software
/// licence's obligations — preserving a copyright header in "the Software",
/// marking modified files, the patent grant — describe nothing a will template
/// does. Attribution is the obligation that fits a drafted document, so the
/// prose carries the licence written for exactly that.
///
/// The carve-out inside the carve-out matters just as much: the blank
/// government PDFs under `templates/forms/` are the issuing agency's work. A
/// CC BY grant over a Nevada state form would claim a copyright the Firm does
/// not hold, and an over-claim in a law firm's own licence file is the kind of
/// error that is quoted back.
#[test]
fn the_legal_prose_carries_its_own_licence() {
    let license = read("LICENSE.md");
    assert!(
        license.contains(CONTENT_LICENSE),
        "LICENSE.md must name `{CONTENT_LICENSE}` as the licence over the \
         drafted prose in `templates/`"
    );

    let flat = license.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("templates/"),
        "LICENSE.md must say which tree the content licence covers"
    );
    assert!(
        flat.contains("templates/forms/"),
        "LICENSE.md must carve out the government forms under \
         `templates/forms/` — they are the issuing agency's work and the Firm \
         grants nothing in them"
    );

    // The tree the split describes has to exist, or the licence is describing
    // a layout the repository no longer has.
    assert!(
        repo_root().join("templates").is_dir(),
        "LICENSE.md licenses `templates/` separately; that tree must exist"
    );
    assert!(
        repo_root().join("templates/forms").is_dir(),
        "LICENSE.md carves out `templates/forms/`; that tree must exist"
    );

    // The tree states its own terms, because someone reading a notation is
    // usually inside `templates/` and not at the repository root.
    let templates_readme = read("templates/README.md");
    assert!(
        templates_readme.contains(CONTENT_LICENSE) || templates_readme.contains("CC BY 4.0"),
        "templates/README.md must state the content licence where an author of \
         a notation will actually see it"
    );
}

/// Every published image declares the licence and carries its text.
///
/// A container image someone pulled is a copy, and its holder has neither the
/// repository nor a release archive. MIT conditions its permission on the
/// notice travelling with every copy, and Apache-2.0 § 4(a) obliges a
/// redistributor to hand recipients the License. Two mechanisms, because they
/// serve different readers: the OCI label is what a registry page shows before
/// anyone pulls, and the staged files are what a running container can actually
/// be made to print.
///
/// `Containerfile.runner` is exempt. It is the CI runner image rather than a
/// published artifact of the software, and it has no distroless runtime stage
/// to stage anything into.
#[test]
fn every_published_image_declares_the_licence_and_stages_its_text() {
    let images = repo_root().join("images");
    let mut offenders = Vec::new();

    for entry in fs::read_dir(&images).expect("read images/").flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("Containerfile.") || name == "Containerfile.runner" {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read Containerfile");

        for required in [
            &format!("org.opencontainers.image.licenses=\"{LICENSE}\"") as &str,
            "COPY LICENSE.md",
            "COPY LICENSE-MIT",
            "COPY LICENSE-APACHE",
        ] {
            if !body.contains(required) {
                offenders.push(format!("{name}: missing `{required}`"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "every published image must declare {LICENSE} and carry every licence \
         text; a puller holds no repository and no archive:\n  {}",
        offenders.join("\n  ")
    );
}

/// The GHCR mirror is opt-in, and the scope it needs is granted per job.
///
/// **Gated.** Every mirror step is conditioned on `vars.GHCR_PUBLISH`, so a
/// deployment that has not set it publishes exactly what it publishes today. An
/// ungated login would fail the publish job, and this workflow's failure mode is
/// a release that looks fine until someone checks the registry days later.
///
/// **Scoped to one job.** `packages: write` belongs on `publish-service` alone.
/// Granting it at the top of the workflow would hand the scope to every job in
/// the file — including the ones that check out and build arbitrary release
/// code — for the benefit of a single optional step.
#[test]
fn the_image_mirror_is_opt_in_and_scoped_to_the_publish_job() {
    let workflow = read(".github/workflows/deploy.yml");

    for required in [
        "registry: ghcr.io",
        "if: vars.GHCR_PUBLISH == 'true'",
        "password: ${{ secrets.GITHUB_TOKEN }}",
        "if [ \"${{ vars.GHCR_PUBLISH }}\" = \"true\" ]; then",
    ] {
        assert!(
            workflow.contains(required),
            "deploy.yml must retain the gated GHCR mirror contract `{required}`"
        );
    }

    // The scope must be granted, and granted narrowly. A `permissions:` block
    // at column 2 is the workflow's; at column 4 it is a job's.
    let granted: Vec<&str> = workflow
        .lines()
        .filter(|line| line.trim() == "packages: write")
        .collect();
    // GHCR is this branch's image registry rather than an opt-in mirror on one
    // job, so both jobs that push an image hold the scope: `publish-service`
    // and `publish-triggers`. What must not change is that each grant is a
    // JOB's — the count is allowed to track the publish jobs, the indent is not.
    assert_eq!(
        granted.len(),
        2,
        "`packages: write` is expected on the two publish jobs, no others"
    );
    for grant in &granted {
        assert!(
            grant.starts_with("      packages: write"),
            "`packages: write` must be granted at job level (six-space indent), \
             not at the top of the workflow where every job would inherit it"
        );
    }

    // One build, both registries. `metadata-action` fans the tags across every
    // image it is given, so the push step must read the composed list — reading
    // the single Artifact Registry name would silently drop the mirror while
    // every gate stayed green.
    assert!(
        workflow.contains("images: ${{ steps.imgs.outputs.list }}"),
        "the publish job's metadata step must read the composed registry list, \
         so one build pushes both registries"
    );
}

/// Public surfaces that name the NEON LAW registration attribute it to the Firm.
///
/// U.S. Reg. No. 6,325,650 belongs to the Firm, and the Firm licenses it to the
/// Neon Law Foundation for its charitable work. A trademark notice that names
/// the wrong owner is worse than none at all, because it is the notice a reader
/// relies on for permission — and under an outbound grant that reliance is no
/// longer hypothetical, since the licence invites forks and the mark is the one
/// thing it withholds from them. The registration number is the anchor: a
/// surface may mention the mark without it, but a surface that cites the number
/// is making an ownership claim and must make the right one.
///
/// Note that this asserts `REGISTRANT` and not `OWNER`. The Foundation holds
/// the copyright and the Firm holds the mark, so a notice that named the
/// copyright holder here would be handing a fork permission nobody gave it.
#[test]
fn trademark_notices_name_the_firm_as_the_registrant() {
    /// The registration itself, used as the anchor for "this line claims
    /// ownership" rather than merely naming the brand.
    const REGISTRATION: &str = "6,325,650";

    let mut offenders = Vec::new();
    for rel in [
        "LICENSE.md",
        "README.md",
        "docs/glossary.md",
        // Where the ownership claim actually lives. `LICENSE.md` names the
        // mark but not the registration, so this is the surface that makes
        // the numbered claim the licences deliberately do not grant.
        "docs/licensing.md",
        "templates/README.md",
        // One binary serves the firm at the root and the Foundation under
        // `/foundation`, so one bundled terms file carries the citation for
        // both faces.
        "neon/content/terms.md",
        "neon/content/terms.md",
    ] {
        // Prose wraps at the Markdown line width, so a claim routinely straddles
        // a line break, and "U.S. Reg. No." defeats splitting on sentence ends.
        // Collapse whitespace and read a window around each citation instead.
        let flat = read(rel).split_whitespace().collect::<Vec<_>>().join(" ");
        let citations: Vec<usize> = flat.match_indices(REGISTRATION).map(|(i, _)| i).collect();
        assert!(
            !citations.is_empty(),
            "{rel} cites the NEON LAW registration and is guarded here; if the \
             citation moved, move this list with it"
        );
        for at in citations {
            let claim = window(&flat, at, REGISTRATION.len());
            if !claim.contains(REGISTRANT) {
                offenders.push(format!(
                    "{rel}: cites {REGISTRATION} without naming `{REGISTRANT}` — …{claim}…"
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "NEON LAW is registered to {REGISTRANT}; these notices say otherwise:\n  {}",
        offenders.join("\n  ")
    );
}

/// Affero / AGPL must not appear as this workspace's grant of record.
///
/// Unrelated to the dual grant and unchanged by it: nothing here is or was
/// AGPL. The
/// guard exists because a generated or pasted header is the easy way for a
/// copyleft grant to land on a workspace that has deliberately chosen a
/// different one — and now that the tree is public, a stray AGPL header is a
/// claim a downstream reader may actually act on.
#[test]
fn affero_grant_appears_on_no_workspace_surface() {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !matches!(
                    name.as_ref(),
                    "target" | ".git" | "node_modules" | ".worktrees"
                ) {
                    walk(&path, out);
                }
            } else if let Ok(body) = fs::read_to_string(&path) {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Tests that forbid Affero may still name it; third-party notes too.
                //
                // `THIRD-PARTY-NOTICES.txt` reproduces other projects' licence
                // texts verbatim, and several of them name the AGPL without
                // being it: MPL-2.0's secondary-licence clause lists it by
                // name, and `aws-lc-sys` and `typst-assets` each bundle a
                // collection of licences for their vendored components. None of
                // that is *this workspace* claiming an Affero grant, which is
                // what this guard exists to catch.
                if matches!(
                    name,
                    "license_of_record.rs" | "routes.rs" | "THIRD-PARTY-NOTICES.txt"
                ) || path.ends_with("docs/multi-cloud.md")
                    || is_sops_ciphertext(name)
                {
                    continue;
                }
                for (i, line) in body.lines().enumerate() {
                    let lower = line.to_lowercase();
                    if lower.contains("agpl") || lower.contains("affero") {
                        out.push(format!("{}:{}", path.display(), i + 1));
                    }
                }
            }
        }
    }

    let mut hits = Vec::new();
    walk(&repo_root(), &mut hits);
    assert!(
        hits.is_empty(),
        "workspace must not claim Affero/AGPL as its own grant; hits:\n  {}",
        hits.join("\n  ")
    );
}
