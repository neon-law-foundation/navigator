//! Pin the licence of record: two organizations, one outbound grant, and no
//! way back.
//!
//! Root `LICENSE` governs the work. Everything the Foundation can license —
//! the Rust workspace, the `navigator` CLI, the build and deployment tooling,
//! and the drafted legal prose under `templates/` — is `AGPL-3.0-only`. One
//! grant, one file, no per-tree exception to look up.
//!
//! The Affero clause is the point rather than a detail. Section 13 obliges
//! anyone who modifies this software and lets users interact with it remotely
//! to offer those users the corresponding source, and a legal-services portal
//! run for other people is exactly that deployment shape. A migration that
//! landed the SPDX tag but lost § 13 would keep the label and drop the
//! obligation, so the section is asserted by name below.
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
//! The retired *permissive* grant is guarded just as hard, and in the opposite
//! direction from everything else here. `MIT OR Apache-2.0` was this
//! workspace's grant, and every copy taken under it keeps it — that history is
//! real and nothing here revokes it. What must not survive is a surface still
//! *offering* it, because a stale `LICENSE-MIT` link or a manifest reading
//! `MIT OR Apache-2.0` would tell a new reader they may take the code
//! permissively today, which is a grant the Foundation no longer makes.
//!
//! The trademark reservation is guarded just as hard, and for a reason the
//! copyright grant does not cover. Copyleft invites forks too, and a fork
//! wearing the operating firm's name would misdirect the one person least able
//! to check who is accountable for their legal work. The marks are the only
//! thing this repository withholds, so a notice that goes missing or names the
//! wrong registrant is the failure that matters most.
//!
//! Structure only, never prose. The wording is expected to keep moving; only a
//! change to the *structure* — the owner changing, a manifest drifting off the
//! tag, the grant file disappearing, the reservation going missing — lands here.

mod common;
use common::is_sops_ciphertext;

use std::fs;
use std::path::{Path, PathBuf};

/// The SPDX expression every manifest in the workspace carries.
///
/// `-only` and never `-or-later`: the terms this repository publishes under are
/// the terms in its own licence file, and no future FSF revision moves them.
const LICENSE: &str = "AGPL-3.0-only";

/// The single licence file. One grant over the whole tree means one instrument,
/// and a reader never has to work out which of several applies to the file in
/// front of them.
const LICENSE_FILE: &str = "LICENSE";

/// The copyright holder: the organization that *produces* this software and
/// makes the outbound grant.
///
/// A rename edits this constant and root `LICENSE` together, and nothing else
/// in this file. It is the legal person rather than the trade name on purpose:
/// a copyright notice has to name someone who can hold a copyright, and "Neon
/// Law" alone is a brand.
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

/// Licence files that must not exist, because each contradicts the grant.
///
/// `LICENSE-MIT`, `LICENSE-APACHE`, and `LICENSE.md` are the retired permissive
/// instruments. Their presence would offer a reader a permissive choice the
/// Foundation no longer grants, beside a copyleft file granting something
/// narrower — two instruments over the same bytes, and no way to tell which
/// binds.
///
/// `EULA.md` is the other direction: it withholds the right to redistribute,
/// which the AGPL permits. `LICENSE-BUSL.txt` is a source-available grant with
/// a four-year clock over code published without one.
const RETIRED_LICENSE_FILES: [&str; 7] = [
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "LICENSE.md",
    "EULA.md",
    "LICENSE-BUSL.txt",
    "FIAT_LICENSE.md",
    "NEON_LICENSE.md",
];

/// Vocabulary that would re-offer the retired permissive grant.
///
/// Matched lowercased against whitespace-flattened prose, because these are
/// phrases and the Markdown line width splits phrases.
const RETIRED_PERMISSIVE_GRANT: [&str; 6] = [
    "mit or apache-2.0",
    "license-mit",
    "license-apache",
    "dual-licensed",
    "dual licensed",
    "cc-by-4.0",
];

/// The workspace root (this test crate is `cli`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Whitespace-flattened, lowercased prose. Markdown wraps at the line width, so
/// a raw `contains` on a phrase reads a refilled paragraph as a deleted clause.
fn flat_lower(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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

/// Directories that are not this workspace's own surface.
///
/// `worktrees` covers `.worktrees`, `.claude/worktrees`, and `.codex/worktrees`
/// alike. Each holds a *complete other checkout*, so walking in reads another
/// branch's files as if they were this one's — and a branch that predates this
/// migration still carries `LICENSE-MIT`. CI clones fresh and never has them,
/// which is exactly why the failure would only ever reproduce on the machine of
/// whoever is working in a worktree.
fn is_skipped_dir(name: &str) -> bool {
    matches!(name, "target" | ".git" | "node_modules" | "vendor")
        || name.trim_start_matches('.') == "worktrees"
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
                if !is_skipped_dir(name.as_ref()) {
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

/// Root `LICENSE` names the copyright holder and carries the SPDX tag.
#[test]
fn root_license_names_the_owner_and_the_spdx_tag() {
    let license = read(LICENSE_FILE);
    assert!(
        license.contains(&format!("Copyright (C) 2026 {OWNER}")),
        "root {LICENSE_FILE} must carry the copyright line \
         `Copyright (C) 2026 {OWNER}`"
    );
    assert!(
        license.contains(&format!("SPDX-License-Identifier: {LICENSE}")),
        "root {LICENSE_FILE} must carry `SPDX-License-Identifier: {LICENSE}`"
    );
}

/// The grant text ships in full, and it is the Affero one.
///
/// Checked section by section rather than by length. A truncated paste that
/// stopped after the definitions would still look like a licence file, and the
/// sections named here are the ones a reader actually relies on: § 4 obliges a
/// conveyor to hand over this License, § 6 governs conveying a built binary,
/// § 11 is the patent grant, and § 13 is the network clause that makes this the
/// Affero licence rather than the ordinary GPL.
#[test]
fn the_grant_text_ships_and_is_complete() {
    assert!(
        repo_root().join(LICENSE_FILE).exists(),
        "{LICENSE_FILE} is the licence of record and must exist"
    );

    let license = read(LICENSE_FILE);
    for required in [
        "GNU AFFERO GENERAL PUBLIC LICENSE",
        "Version 3, 19 November 2007",
        "TERMS AND CONDITIONS",
        "0. Definitions.",
        "4. Conveying Verbatim Copies.",
        "5. Conveying Modified Source Versions.",
        "6. Conveying Non-Source Forms.",
        "11. Patents.",
        "13. Remote Network Interaction; Use with the GNU General Public License.",
        "15. Disclaimer of Warranty.",
        "END OF TERMS AND CONDITIONS",
    ] {
        assert!(
            license.contains(required),
            "{LICENSE_FILE} must carry the verbatim AGPL-3.0 text; `{required}` \
             is missing, so the grant it publishes is incomplete"
        );
    }
}

/// The network clause is stated where a deployer will read it.
///
/// § 13 is the whole reason this workspace is on the Affero licence rather than
/// a permissive one, and it is the obligation a reader is least likely to expect
/// from the SPDX tag alone. Someone who runs a modified Navigator as a legal
/// portal owes their users the corresponding source, and the licence file has to
/// say so in its own voice — not only inside § 13's own legalese, which a
/// deployer skims past on the way to deciding they may fork.
#[test]
fn the_licence_states_the_network_obligation_in_its_own_voice() {
    let flat = flat_lower(&read(LICENSE_FILE));

    assert!(
        flat.contains("section 13"),
        "{LICENSE_FILE} must name section 13 in its own preamble — the network \
         obligation is the reason this grant is Affero, and a deployer reads the \
         top of the file rather than § 13's own text"
    );
    assert!(
        flat.contains("corresponding source"),
        "{LICENSE_FILE} must say that a modified network deployment owes users \
         the corresponding source"
    );
    assert!(
        flat.contains("remotely"),
        "{LICENSE_FILE} must say the obligation attaches to letting users \
         interact with the software remotely, which is the deployment shape a \
         legal-services portal actually has"
    );
}

/// The marks are reserved, and `LICENSE` says so in the same breath as the
/// grant.
///
/// This is the reservation the copyright grant does not make, and it is the
/// clause most likely to be lost in a rewrite, because every other sentence in
/// the file is about giving things away. A reader deciding whether they may ship
/// a fork called "Neon Law" reads the licence file, so the answer has to be
/// there.
#[test]
fn the_licence_reserves_the_marks_alongside_the_grant() {
    let flat = read(LICENSE_FILE)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(
        flat.contains("rights in copyright, not in trademarks"),
        "{LICENSE_FILE} must state that the grant covers copyright and not \
         trademarks — copyleft invites forks too, and the marks are the only \
         thing this repository withholds from one"
    );
    assert!(
        flat.contains("6,325,650"),
        "{LICENSE_FILE} must cite the NEON LAW registration it reserves"
    );
    assert!(
        flat.contains("views::brand_bundle"),
        "{LICENSE_FILE} must point a fork at the brand manifest — telling \
         someone they may not use the marks without showing them the rename seam \
         leaves patching sources as the obvious move"
    );
}

/// The grant cannot be walked back.
///
/// This is the guard that matters most. A reader who clones this repository
/// holds real rights, and every copy already taken keeps them regardless of what
/// a later commit says.
///
/// So the risk is an accidental *retraction*: a proprietary clause landing in
/// the licence file would leave the repository claiming to be private while its
/// own licence file grants the world a licence. Both cannot be true, and the one
/// people have already relied on is the grant.
#[test]
fn the_license_grants_the_public_something_and_cannot_take_it_back() {
    let license = flat_lower(&read(LICENSE_FILE));

    for required in [
        "gnu affero general public license",
        "free software",
        "redistribute",
    ] {
        assert!(
            license.contains(required),
            "root {LICENSE_FILE} must state `{required}` — the software is \
             published under {LICENSE} and the file has to read as a grant"
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
            "root {LICENSE_FILE} must not retract or narrow the outbound grant; \
             found `{retracted}`. The software is published under {LICENSE} and \
             every copy already taken keeps its rights — a clause like this \
             makes the repository lie about what its readers already hold."
        );
    }
}

/// No retired licence file returns, and `LICENSE` is the only one.
///
/// A returning `LICENSE-MIT` is the live contradiction this guard exists for:
/// it would offer a permissive grant the Foundation no longer makes, beside a
/// copyleft file granting something narrower, with nothing to tell a reader
/// which of the two binds.
#[test]
fn no_retired_license_file_returns() {
    assert!(
        repo_root().join(LICENSE_FILE).exists(),
        "{LICENSE_FILE} is the license of record and must exist"
    );
    for retired in RETIRED_LICENSE_FILES {
        assert!(
            !repo_root().join(retired).exists(),
            "{retired} is retired; {LICENSE} in {LICENSE_FILE} is the only \
             instrument over this work, and a second licence file at the root \
             leaves a reader guessing which one binds"
        );
    }
}

/// Exactly one licence file sits at the repository root.
///
/// The count is the assertion. "One LICENSE file" is the shape this migration
/// chose, and the way it decays is a helpful-looking sibling — `LICENSE.txt`
/// beside `LICENSE`, or a `COPYING` a tool dropped in — rather than one of the
/// specific names `RETIRED_LICENSE_FILES` already knows about.
#[test]
fn the_repository_root_carries_exactly_one_licence_file() {
    let mut found: Vec<String> = fs::read_dir(repo_root())
        .expect("read repository root")
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| {
            let lower = name.to_lowercase();
            ["license", "licence", "copying"]
                .iter()
                .any(|stem| lower.starts_with(stem))
        })
        .collect();
    found.sort();

    assert_eq!(
        found,
        vec![LICENSE_FILE.to_string()],
        "the root must carry exactly one licence file, `{LICENSE_FILE}`; found \
         {found:?}. One grant covers the whole tree, so a second file can only \
         contradict the first."
    );
}

/// No manifest, workflow, or dependency policy still declares a retired tag.
///
/// Two failure directions, one list. A stale `LicenseRef-Proprietary` would
/// tell a downstream reader they hold no rights when in fact they hold the
/// AGPL's; a stale `MIT OR Apache-2.0` would tell them they may take the code
/// permissively, which is a grant the Foundation no longer makes.
#[test]
fn no_surface_still_declares_a_retired_tag() {
    let mut hits = Vec::new();
    for rel in [
        "Cargo.toml",
        "lsp/vscode-ext/package.json",
        "deny.toml",
        ".github/workflows/deploy.yml",
        "README.md",
        "CONTRIBUTING.md",
        "docs/licensing.md",
        LICENSE_FILE,
    ] {
        let path = repo_root().join(rel);
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let flat = flat_lower(&body);
        for stale in [
            "licenseref-proprietary",
            "eula.md",
            "not source-available",
            "access to this repository is not a licence",
            // A manifest on `BUSL-1.1` would tell a downstream reader their
            // rights expire on a clock that does not exist.
            "busl-1.1",
            "license-busl",
        ] {
            if flat.contains(stale) {
                hits.push(format!("{rel}: `{stale}`"));
            }
        }
        for stale in RETIRED_PERMISSIVE_GRANT {
            // `deny.toml` names `MIT` and `Apache-2.0` as *inbound* dependency
            // policy, which is a different question from the outbound grant and
            // stays permissive on purpose. Only the composed dual-grant
            // expression is forbidden there, and the flattened `contains` above
            // already distinguishes them.
            if flat.contains(stale) {
                hits.push(format!("{rel}: re-offers the retired grant `{stale}`"));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "these surfaces still declare a retired licence:\n  {}",
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
    let flat = flat_lower(&contributing);

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
    for rel in ["CONTRIBUTING.md", LICENSE_FILE, "docs/licensing.md"] {
        assert!(
            !read(rel).contains(RETIRED_AGREEMENT),
            "{rel} still cites the retired `{RETIRED_AGREEMENT}`; contributions \
             are inbound = outbound and there is nothing to sign"
        );
    }
}

#[test]
fn readme_states_the_license_of_record() {
    let readme = read("README.md");
    let flat = readme.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains(OWNER),
        "README.md must name `{OWNER}` as the copyright holder of Neon Law Navigator"
    );
    assert!(
        flat.contains(LICENSE),
        "README.md must name `{LICENSE}` as the software's licence"
    );
    for retired in RETIRED_LICENSE_FILES {
        assert!(
            !readme.contains(retired),
            "README.md must not link the retired licence file `{retired}`"
        );
    }
}

/// One grant covers the whole tree, `templates/` included.
///
/// The drafted legal prose used to carry `CC-BY-4.0` separately. It no longer
/// does: a reader of a notation body is under the same licence as a reader of
/// the code that renders it, and `templates/README.md` has to say so where a
/// notation author will actually see it — someone editing a template is inside
/// `templates/`, not at the repository root.
///
/// The carve-out inside the tree survives, because it is not a licensing choice
/// at all: the blank government PDFs under `templates/forms/` are the issuing
/// agency's work. An AGPL grant over a Nevada state form would claim a copyright
/// the Foundation does not hold, and an over-claim in a law firm's own licence
/// file is the kind of error that is quoted back.
#[test]
fn the_single_grant_covers_the_templates_tree() {
    let license = read(LICENSE_FILE);
    let flat = license.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(
        flat.contains("templates/"),
        "{LICENSE_FILE} must say that the grant reaches `templates/` — the \
         drafted prose is no longer licensed apart from the software, and \
         silence there is what sends a reader looking for a second instrument"
    );
    assert!(
        flat.contains("templates/forms/"),
        "{LICENSE_FILE} must carve out the government forms under \
         `templates/forms/` — they are the issuing agency's work and the \
         Foundation grants nothing in them"
    );

    // The tree the licence describes has to exist, or the licence is describing
    // a layout the repository no longer has.
    assert!(
        repo_root().join("templates").is_dir(),
        "{LICENSE_FILE} names `templates/`; that tree must exist"
    );
    assert!(
        repo_root().join("templates/forms").is_dir(),
        "{LICENSE_FILE} carves out `templates/forms/`; that tree must exist"
    );

    // The tree states its own terms, because someone reading a notation is
    // usually inside `templates/` and not at the repository root.
    let templates_readme = read("templates/README.md");
    assert!(
        templates_readme.contains(LICENSE),
        "templates/README.md must state `{LICENSE}` where an author of a \
         notation will actually see it"
    );
    let flat_templates = flat_lower(&templates_readme);
    for stale in RETIRED_PERMISSIVE_GRANT {
        assert!(
            !flat_templates.contains(stale),
            "templates/README.md still offers the retired grant `{stale}`; the \
             prose here is under {LICENSE} with the rest of the tree"
        );
    }
}

/// Every published image declares the licence and carries its text.
///
/// A container image someone pulled is a copy, and its holder has neither the
/// repository nor a release archive. AGPL § 4 conditions the permission to
/// convey on handing every recipient this License along with the work, and § 13
/// may oblige that holder to pass the source on in turn — which they cannot do
/// from terms they were never shown. Two mechanisms, because they serve
/// different readers: the OCI label is what a registry page shows before anyone
/// pulls, and the staged file is what a running container can actually be made
/// to print.
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
            &format!("COPY {LICENSE_FILE} /app/{LICENSE_FILE}") as &str,
        ] {
            if !body.contains(required) {
                offenders.push(format!("{name}: missing `{required}`"));
            }
        }

        // A stale label would advertise a permissive grant on the registry page
        // that the staged text does not make.
        for stale in RETIRED_PERMISSIVE_GRANT {
            if flat_lower(&body).contains(stale) {
                offenders.push(format!("{name}: still declares `{stale}`"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "every published image must declare {LICENSE} and carry the licence \
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
        LICENSE_FILE,
        "README.md",
        "docs/glossary.md",
        // Where the ownership claim actually lives. `LICENSE` names the mark
        // but not every surface does, so this is the doc that makes the
        // numbered claim the licence deliberately does not grant.
        "docs/licensing.md",
        "templates/README.md",
        // One binary serves the firm at the root and the Foundation under
        // `/foundation`, so one bundled terms file carries the citation for
        // both faces.
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

/// No workspace surface still offers the retired permissive grant.
///
/// The inverse of the guard this file used to carry. `MIT OR Apache-2.0` was
/// the grant, and every copy taken under it keeps it — nothing here revokes
/// that history. What must not survive is a surface still *offering* it: a
/// `LICENSE-MIT` link in a doc, a `dual-licensed` sentence in marketing copy, or
/// a generated header pasted from an older commit each tell a reader they may
/// take this code permissively today, which is a grant the Foundation no longer
/// makes. A tree with one copyleft licence file and a dozen permissive claims
/// scattered through its prose has published a contradiction.
///
/// Third-party facts are not claims about this workspace and are exempt:
/// `THIRD-PARTY-NOTICES.txt` reproduces other projects' licence texts verbatim,
/// `docs/multi-cloud.md` records that Garage is AGPL software Navigator runs
/// unmodified, and `vendor/` is somebody else's code entirely.
#[test]
fn no_workspace_surface_re_offers_the_retired_permissive_grant() {
    /// Files that legitimately name the retired grant.
    ///
    /// This test file names it to forbid it, and `cli_downloads.rs` does the
    /// same for the download page's rendered copy — an assertion on absence is
    /// the opposite of an offer, and that render test pins the page far more
    /// precisely than a grep over its source could.
    ///
    /// `THIRD-PARTY-NOTICES.txt` and `Cargo.lock` describe the dependency tree,
    /// where `MIT OR Apache-2.0` is the single most common inbound licence and
    /// says nothing about the outbound grant. `deny.toml` is the inbound
    /// allowlist for the same reason, and `notices.rs` documents how that tree's
    /// texts are rendered.
    fn is_exempt(path: &Path, name: &str) -> bool {
        matches!(
            name,
            "license_of_record.rs"
                | "cli_downloads.rs"
                | "THIRD-PARTY-NOTICES.txt"
                | "Cargo.lock"
                | "deny.toml"
                | "notices.rs"
        ) || is_sops_ciphertext(name)
            || path.ends_with("docs/multi-cloud.md")
            // Third-party font and dependency licence texts vendored verbatim.
            || path.components().any(|c| c.as_os_str() == "gorp-serif")
    }

    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !is_skipped_dir(name.as_ref()) {
                    walk(&path, out);
                }
                continue;
            }
            if is_exempt(&path, name.as_ref()) {
                continue;
            }
            let Ok(body) = fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in body.lines().enumerate() {
                let lower = line.to_lowercase();
                for stale in RETIRED_PERMISSIVE_GRANT {
                    if lower.contains(stale) {
                        out.push(format!("{}:{}: `{stale}`", path.display(), i + 1));
                    }
                }
            }
        }
    }

    let mut hits = Vec::new();
    walk(&repo_root(), &mut hits);
    assert!(
        hits.is_empty(),
        "these surfaces still offer the retired permissive grant; the workspace \
         is {LICENSE} and a stale permissive claim tells a reader they hold \
         rights nobody granted them:\n  {}",
        hits.join("\n  ")
    );
}
