---
kind: filing
title: Source Code Preservation TRO (California)
respondent_type: person_and_entity
code: source_code_preservation_tro__california
jurisdiction: CA
confidential: true
prompts:
  client: What is the moving party's full legal name, as it appears in the caption?
  contact: >-
    Who will sign the engineering declaration? Give their name, title, employing entity, the period during which they
    held engineering responsibility, and the scope of that responsibility.
custom_questions:
  repository_inventory:
    prompt: >-
      List every source code repository to be preserved. For each one give the exact repository name, the hosting
      platform, the owning organization or account, the URL, the default branch, the approximate commit count, the
      earliest and most recent commit dates, the business function it performs, and the earnout metric that depends
      on it. Rule 65(d)(1)(C) requires this detail; a general reference to "source code" is not enforceable.
  custody_transfer:
    prompt: >-
      Describe how the repositories came under the responding party's control after closing — organization
      migration, single sign-on enforcement, administrative role changes, or account consolidation — and identify
      who now holds administrative rights.
  custody_transfer_date:
    prompt: On what date did the responding party take administrative control of the repositories?
  independent_backup:
    prompt: >-
      Does the moving party hold any complete, independent copy, mirror, or backup of the repositories outside the
      responding party's control?
    choices:
      none: No copy of any kind exists outside the responding party's control
      partial: A partial or stale copy exists, and its scope and date are described below
      complete: A complete independent copy exists
  imminence_basis:
    prompt: >-
      What is the evidentiary basis for the risk of destruction? This answer sets how the irreparable-harm section
      is argued, so choose the strongest option the record actually supports.
    choices:
      stated_threat: A communication, notice, or plan stating that the repositories will be deleted or retired
      access_revoked: Access was revoked or reduced, so continued existence can no longer be verified
      migration: A migration, consolidation, or decommissioning effort is under way
      inferred: No specific act or statement — risk is inferred from exclusive custody plus the dispute
  threat_evidence:
    prompt: >-
      Describe every document or communication bearing on the risk of destruction — migration plans,
      decommissioning notices, archival announcements, access revocations, license or seat reductions, offboarding
      tickets, or statements about retiring the acquired technology stack. Enter "none located" if there are none.
  litigation_hold_date:
    prompt: On what date was the litigation hold letter sent to the responding party or its counsel?
  personnel_relief:
    prompt: >-
      Does the record prove the employing entity, current duties, threatened action, decisionmaker, and specific
      metric effect for the individual whose role the order would protect?
    choices:
      proven: Yes — include the personnel paragraphs
      unproven: No — omit the personnel paragraphs and proceed on preservation alone
  protected_duties:
    prompt: >-
      If the personnel paragraphs are included, enumerate the existing duties and access to be preserved and the
      specific earnout metric each one serves.
  hearing_date:
    prompt: On what date and at what time will the motion be heard?
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: entity__company
  entity__company:
    _: project__engagement
  project__engagement:
    _: custom_text__repository_inventory
  custom_text__repository_inventory:
    _: person__contact
  person__contact:
    _: custom_text__custody_transfer
  custom_text__custody_transfer:
    _: custom_datetime__custody_transfer_date
  custom_datetime__custody_transfer_date:
    _: custom_single_choice__independent_backup
  custom_single_choice__independent_backup:
    _: custom_single_choice__imminence_basis
  custom_single_choice__imminence_basis:
    _: custom_text__threat_evidence
  custom_text__threat_evidence:
    _: custom_datetime__litigation_hold_date
  custom_datetime__litigation_hold_date:
    _: custom_single_choice__personnel_relief
  custom_single_choice__personnel_relief:
    _: custom_text__protected_duties
  custom_text__protected_duties:
    _: custom_datetime__hearing_date
  custom_datetime__hearing_date:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__code_inventory
  intake_persisted__code_inventory:
    filing_rendered: lawyer_review
  lawyer_review:
    approved: generate_pdf__tro_pdf
    changes_requested: reask__code_inventory
    rejected: END
  reask__code_inventory:
    intake_resubmitted: lawyer_review
  generate_pdf__tro_pdf:
    pdf_persisted: END
  END: {}
---

# Motion for a temporary restraining order preserving source code

`{{person__client.name}}` and `{{entity__company.name}}` (together, "Plaintiffs") move under Federal Rule of Civil
Procedure 65 and Civil Local Rule 65-1 for a temporary restraining order preserving the source code repositories
identified below, and for an order to show cause why a preliminary injunction should not issue, in aid of the parties'
arbitration in the matter referred to as `{{project__engagement.name}}`.

This motion is made **on notice**. Plaintiffs do not seek relief ex parte and do not contend that the Rule 65(b)(1)
standard for relief without notice is satisfied on the present record. The motion will be heard on
`{{custom_datetime__hearing_date}}`.

## What the order would and would not do

The requested order prohibits the deliberate acts by which a codebase and its history are destroyed: deleting a
repository, rewriting or force-overwriting its history, terminating the account that holds it, and purging its backups.

The requested order does **not** restrict ordinary software development. Engineers may write code, commit, branch,
merge, tag, build, test, deploy, refactor, and delete files from a working tree. It does not require the responding
party to develop, lawyer, fund, or prioritize any product, and it does not restrict reorganization of the acquired
business.

## The defined term

**"Code Assets"** means, collectively: the source code repositories enumerated in Schedule A; the complete
version-control history of each, including all commits, commit metadata, branches, tags, refs, and reflogs; the hosting
accounts, organizations, and workspaces in which those repositories reside; continuous-integration and deployment
configurations, pipeline definitions, and infrastructure-as-code held in or referenced by those repositories; container
and package registries and build artifacts generated from them; technical documentation, architecture records, and
runbooks maintained alongside them; all backups, snapshots, mirrors, forks, and archival copies of the foregoing,
wherever stored; and the credentials, access-control lists, and permission settings governing access to the foregoing.

## Schedule A — the repositories

`{{custom_text__repository_inventory}}`

## Custody

The Code Assets came under the responding party's control on `{{custom_datetime__custody_transfer_date}}`.
`{{custom_text__custody_transfer}}`

On the question of an independent copy outside the responding party's control, the record is:
`{{custom_single_choice__independent_backup}}`. Where no such copy exists, Plaintiffs cannot detect deletion, cannot
verify continued existence, and could not restore the Code Assets if they were destroyed.

## The basis for the risk of destruction

The evidentiary posture is: `{{custom_single_choice__imminence_basis}}`.

The documents and communications bearing on that risk are: `{{custom_text__threat_evidence}}`.

Plaintiffs state the posture plainly rather than overstating it. Where the risk is inferred rather than announced, the
inference rests on structure: the responding party holds exclusive custody of assets that are at once the disputed
performance instrument and the proof of the disputed conduct, the parties are now adverse, and destruction would be
irreversible and undetectable by Plaintiffs until too late.

## Why the harm is irreparable

Irreparable harm must be proven, not presumed. *Herb Reed Enterprises, LLC v. Florida Entertainment Management, Inc.*,
736 F.3d 1239, 1249–51 (9th Cir. 2013). Plaintiffs prove it by the nature of the asset rather than by speculation.

First, the harm is definitionally unrecoverable. A deleted repository and its commit history cannot be reconstructed
from money. There is no market from which a replacement can be purchased and no expert who can testify to what a
destroyed history contained.

Second, the harm is doubled. The Code Assets are simultaneously the instrument by which the contingent consideration
must be earned and the contemporaneous, machine-generated evidence of the conduct at issue. One act destroys both.

Third, the harm is undetectable in time. A party holding no independent copy and no administrative access learns of
destruction only after it is complete and unwindable.

Fourth, the post-hoc remedy is inadequate. Rule 37(e) permits sanctions for lost electronically stored information, but
the findings of prejudice or intent it requires are difficult for a party without access to the destroyed material to
establish. An adverse-inference instruction is a poor substitute for the code itself. Prevention is the only adequate
remedy, which is when equitable relief is warranted.

## Why compliance costs the responding party nothing

This is the decisive factor. Modern version control is additive by design. Each change is recorded as a new commit
appended to the history, and prior commits remain retrievable. Routine engineering work — branching, merging,
deploying, refactoring, and deleting files from the current working tree — leaves the historical record intact. A file
deleted today remains recoverable from the commit in which it last existed.

Destroying that record requires a separate and deliberate act outside ordinary practice: deleting the repository,
deleting the organization or account that owns it, force-pushing a rewritten history, running a history-rewriting
operation across published history, deleting branches or tags and allowing the associated objects to expire, or
deleting backups and mirrors. Each requires elevated permissions. None occurs by accident.

Those acts, and only those acts, are what the order restrains. An engineering organization instructed not to perform
them experiences no impact on its ability to develop, test, deploy, or operate the platform.

The declaration of `{{person__contact.name}}` establishes these propositions as technical fact rather than as argument
of counsel.

## The order codifies a duty that already exists

The duty to preserve attached when this litigation became reasonably foreseeable, no later than
`{{custom_datetime__litigation_hold_date}}`, when the litigation hold letter issued. Rule 37(e) presupposes that duty.
The proposed order does not enlarge it. It identifies the specific assets covered and makes the obligation enforceable
through contempt rather than through a sanctions motion litigated after the material is gone.

A responding party that intends to comply with its preservation duty loses nothing by being ordered to do so.

## Relief requested

Plaintiffs request an order, pending the preliminary-injunction hearing or earlier action by the arbitral tribunal,
that:

1. restrains the responding party and those acting with it from deleting, destroying, erasing, wiping, purging,
   archiving into an inaccessible state, or permitting the lapse or expiration of any Code Asset;
2. restrains rewriting, truncating, squashing, or force-overwriting the version-control history of any repository
   within the Code Assets, including by force-push, history rewrite, branch deletion, tag deletion, or reflog
   expiration;
3. restrains terminating, downgrading, suspending, or allowing the lapse of any hosting account, organization,
   subscription, or license whose termination would render any Code Asset inaccessible or subject to automatic
   deletion, and restrains transfer of any Code Asset outside the responding party's custody without advance written
   notice to Plaintiffs' counsel;
4. restrains deleting or allowing the expiration of any backup, snapshot, mirror, or archival copy of any Code Asset,
   and requires suspension of any automated retention, rotation, or deletion policy that would cause such deletion;
5. expressly permits ordinary development and operations, so that nothing in the foregoing paragraphs restricts
   creating, modifying, or deleting files within a working tree, creating, merging, and closing branches and pull
   requests, or committing, tagging, building, testing, deploying, and operating the platform, provided the historical
   record remains retrievable;
6. requires the responding party to file and serve, within a set number of court days, a declaration from a person
   with knowledge confirming that each repository in Schedule A presently exists, stating its current commit count and
   most recent commit date, identifying its owning account, and describing the preservation measures implemented;
7. requires preservation of records concerning the Code Assets, including access logs, permission-change records,
   administrative actions, migration and decommissioning plans, retention-policy changes, and communications about
   consolidating, retiring, or archiving the acquired technology stack; and
8. requires Plaintiffs promptly to commence or continue the agreed arbitral process and to notify the arbitral
   institution of the court application.

## The secondary personnel relief

The record on the individual whose role the order would protect is: `{{custom_single_choice__personnel_relief}}`.

Where that showing is made, Plaintiffs additionally request an order restraining the responding party from terminating,
demoting, suspending, or removing that individual from the following duties and access, where the action would impede
or impair the identified metric: `{{custom_text__protected_duties}}`. That paragraph does not prevent action for
documented good cause unrelated to the earnout or this dispute.

Personnel losses standing alone are ordinarily compensable and do not justify emergency relief. *Sampson v. Murray*,
415 U.S. 61, 90 (1974). The theory here is not lost employment but the destruction of a one-time measurement
opportunity that depends on identified work only that individual performs. Where the record does not establish the
employing entity, current duties, threatened action, decisionmaker, and specific metric effect, these paragraphs are
omitted — the preservation relief is stronger without a weak personnel request attached to it.

## Specificity and security

Rule 65(d) requires the injunction to state its terms specifically and to describe in reasonable detail the restrained
acts. The proposed order enumerates each repository by name, platform, owning account, and URL in Schedule A;
identifies each restrained act by its technical name; specifies the verification declaration and its deadline; and
sunsets on a fixed event. A reader can determine from the four corners of the order exactly what is forbidden.

Where the governing agreement provides that equitable relief may issue without bond, Plaintiffs request enforcement of
that agreement. If the Court concludes Rule 65(c) requires security notwithstanding the waiver, nominal security is
appropriate, because an order to refrain from destroying files creates no cognizable loss.

## Conclusion

The Court should enter the temporary restraining order preserving the Code Assets, issue an order to show cause on an
expedited schedule, and hold the status quo until the Court or the arbitral tribunal can act on a complete record.
