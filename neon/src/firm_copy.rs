//! The firm's public marketing copy.
//!
//! Neon Law's own words about its practice, owned by the binary that publishes
//! them rather than by the application underneath.

/// The firm's public page copy.
///
/// The firm builds Navigator for its own practice and uses it while
/// co-counseling cases with the Neon Law Foundation. The `/navigator` page
/// makes one invitation: co-counsel a pro bono case together.
///
/// [`legal_services`] is the priced one: the routine consumer work — a will, a
/// trust, a name change, a formation — with the firm's actual fee printed
/// beside each entry. That is the page's whole reason to exist. A person
/// deciding whether they can afford a lawyer gets an answer from the page
/// rather than from a consultation, and a firm that publishes its fees cannot
/// quietly charge one client more than another for the same work.
use webapp::foundation_marketing::{Band, Card, PageContent, Run, Step};

/// One published flat fee: what the matter is, what it costs, and what that
/// figure does and does not cover.
///
/// The scope is not decoration. A fee shown bare reads as "everything this
/// matter could need", so a filing fee billed separately afterwards arrives
/// as a surprise charge from a firm that advertised a fixed price. Every
/// entry states its own boundary, and the footer disclaimer states the
/// general one.
struct FlatFee {
    matter: &'static str,
    /// The published fee, already formatted for the page, or `None` while
    /// the firm has not set one.
    ///
    /// A string rather than a number because some entries carry a
    /// qualifier a number cannot ("+ state fee"), and a price a reader
    /// sees must be the exact string the firm chose rather than something
    /// a formatter derived.
    ///
    /// `None` renders no chip at all rather than "contact us" or a dash. A
    /// placeholder in a price column reads as a price the reader failed to
    /// understand; an absent one reads as absent. Every entry is `None`
    /// today: the schedule's shape is settled and its figures are a
    /// decision for the firm, not a value to be invented here.
    fee: Option<&'static str>,
    scope: &'static str,
}

/// The firm's published fee schedule.
///
/// Ordered as a person meets these matters rather than by price: the
/// estate documents first, because that is the largest share of what
/// walks in; then the personal filings; then the small-business work.
///
/// **Every figure here is a published commitment.** Setting one tells the
/// public what the firm charges, and a fee a client has read cannot be
/// quietly revised upward for them — so a number here is a decision by the
/// firm, never a copy edit and never a placeholder someone forgot to
/// replace. That is why they are all `None` today.
///
/// When they are set: third-party fees — the Secretary of State's, the
/// IRS's, the USPTO's, the court's — are never folded into a figure here.
/// They are set by someone else and change without asking us, so a number
/// that silently included one would go wrong on its own. Write those as
/// `$X + state fee`, which `a_fee_with_a_pass_through_names_it` enforces.
const FLAT_FEES: &[FlatFee] = &[
    FlatFee {
        matter: "Simple will",
        fee: None,
        scope: "One will, drafted from your answers and reviewed by a licensed attorney, \
                through signing and witnessing.",
    },
    FlatFee {
        matter: "Estate package",
        fee: None,
        scope: "A will, a financial power of attorney, and a healthcare directive, drafted \
                together so they agree with one another.",
    },
    FlatFee {
        matter: "Revocable living trust",
        fee: None,
        scope: "The trust, a pour-over will, and the deed transferring one Nevada property \
                into it. Further properties are quoted.",
    },
    FlatFee {
        matter: "Uncontested name change",
        fee: None,
        scope: "The petition, the publication notice, and the hearing. Court filing and \
                publication costs are billed at cost.",
    },
    FlatFee {
        matter: "Demand letter",
        fee: None,
        scope: "One letter over the firm's signature, after we read what you have. It is \
                not a retainer to litigate if the letter does not work.",
    },
    FlatFee {
        matter: "Tenant eviction defense",
        fee: None,
        scope: "The answer and one hearing in a Nevada summary eviction. An appeal or a \
                contested trial is a separate engagement.",
    },
    FlatFee {
        matter: "LLC formation",
        fee: None,
        scope: "Articles, an operating agreement, the EIN, and the initial state filing. \
                The Secretary of State sets its own fee.",
    },
    FlatFee {
        matter: "Nonprofit formation and 501(c)(3)",
        fee: None,
        scope: "Articles, bylaws, a conflict-of-interest policy, and the Form 1023 \
                application. The IRS sets its own user fee.",
    },
    FlatFee {
        matter: "Trademark application",
        fee: None,
        scope: "A clearance search and one class of one application, through filing. The \
                USPTO sets its own per-class fee.",
    },
    FlatFee {
        matter: "Nevada annual report",
        fee: None,
        scope: "The annual list and the state business licence renewal for one entity. The \
                state sets its own fee.",
    },
    FlatFee {
        matter: "Mutual NDA review",
        fee: None,
        scope: "One agreement read and redlined, with a short note on what we changed and \
                why.",
    },
];

/// `/navigator` — why the firm builds Navigator and the co-counsel invitation.
pub fn navigator() -> PageContent {
    PageContent {
        head_title: format!(
            "Neon Law Navigator — {}",
            views::brand::FIRM_BRAND.site_name
        ),
        meta_description: "Neon Law Navigator is Neon Law's legal project platform \
                           for accurate, expedient matter resolution."
            .to_string(),
        title: "Neon Law Navigator".to_string(),
        tagline: "Our legal project platform to empower everyone to be a vibe coder.".to_string(),
        bands: vec![
            navigator_purpose_band(),
            Band::Cta {
                heading: "Co-Counsel a Pro Bono Case with us and the Neon Law Foundation"
                    .to_string(),
                body: Some(
                    "To see how vibe coding can help you tell more persuasive stories, we \
                     invite you to help make the world a better place and explore AI together."
                        .to_string(),
                ),
                email: views::brand::firm_email().to_string(),
                email_subject: Some("Co-Counseling for Good with AI".to_string()),
            },
        ],
    }
}

/// The firm's client-serving purpose for Navigator.
fn navigator_purpose_band() -> Band {
    Band::Statement {
        heading: "Why we build it".to_string(),
        lead: "We build Navigator for the purpose of serving clients as expeditiously, \
               precisely, accurately, and in alignment with their interests."
            .to_string(),
        body: vec![],
    }
}

/// `/services` — the published flat-fee schedule.
///
/// The routine end of the practice that is neither a dispute nor ongoing
/// counsel: the one-time consumer matters a person actually walks in with.
/// Every one carries its fee.
///
/// Publishing them is the decision this page embodies. A prospective client
/// who has been told all their life that a lawyer is unaffordable will not
/// book a consultation to find out; they will assume the answer and not
/// call. A number on the page answers that before the conversation, and it
/// binds the firm to charge the same person the same amount for the same
/// work, which is the part that makes it fair rather than merely
/// convenient.
///
/// Litigation and fractional general counsel are deliberately absent. Their
/// scope is not knowable in advance, so a published figure there would be
/// either a guess or a floor dressed as a price; both pages quote through
/// `/contact` and say so.
pub fn legal_services() -> PageContent {
    PageContent {
        head_title: format!("Legal Services — {}", views::brand::FIRM_BRAND.site_name),
        meta_description: "Flat-fee legal services from Neon Law: wills, trusts, name \
                           changes, formations, trademarks, and tenant defense, each on a \
                           fixed fee and reviewed by a licensed attorney."
            .to_string(),
        title: "Legal Services".to_string(),
        tagline: "One scope, one fee, agreed before we start.".to_string(),
        bands: vec![
            legal_services_intro_band(),
            legal_services_fee_band(),
            legal_services_steps_band(),
            legal_services_cta_band(),
        ],
    }
}

/// The fee schedule itself: one card per matter, its price in the chip row.
///
/// A card rather than a table because the scope line has to travel with the
/// figure. A price list that showed only names and numbers would be read as
/// a quote for whatever the reader has in mind, and the boundary — one
/// property, one class, one hearing — is the difference between a fee the
/// firm can honour and one it will have to walk back.
fn legal_services_fee_band() -> Band {
    Band::Cards {
        anchor: "fees".to_string(),
        overline: "Flat fees".to_string(),
        heading: "The work we do at a flat fee".to_string(),
        description: Some(
            "Each of these is a fixed-fee matter: one scope, one price, agreed before we \
             start. Where a government body charges its own fee we pass it through at cost — \
             we do not mark it up, and we cannot control it. Email us for the fee on the \
             matter you need."
                .to_string(),
        ),
        items: FLAT_FEES
            .iter()
            .map(|entry| Card {
                title: entry.matter.to_string(),
                // No chip at all while the fee is unset. An empty chip
                // would render as a blank price tag, which reads worse
                // than no price tag.
                chips: entry.fee.map(str::to_string).into_iter().collect(),
                body: vec![vec![Run::plain(entry.scope)]],
                href: None,
                href_label: None,
            })
            .collect(),
    }
}

/// The closing call to action: contact the firm to get started.
fn legal_services_cta_band() -> Band {
    Band::Cta {
        heading: "Ready to get started?".to_string(),
        body: Some(
            "Tell us which matter you need and we will send you the flat fee for it before \
             any work begins. Email the firm to start."
                .to_string(),
        ),
        email: views::brand::firm_email().to_string(),
        email_subject: Some("Legal Services".to_string()),
    }
}

/// The line under the tagline: who the schedule is for, and the one
/// engagement it does not apply to.
fn legal_services_intro_band() -> Band {
    Band::Statement {
        heading: "Who this is for".to_string(),
        lead: String::new(),
        body: vec![vec![
            Run::plain(
                "These fees are for one-time matters. Business filings are already included \
                 in our ",
            ),
            Run::link("fractional GC", "/fractional-gc"),
            Run::plain(" projects, and a dispute is "),
            Run::link("litigation", "/litigation"),
            Run::plain(", which we quote per engagement."),
        ]],
    }
}

/// How the engagement runs — a short, fast, account-driven process, with a
/// licensed attorney's review before anything is filed.
fn legal_services_steps_band() -> Band {
    Band::Steps {
        anchor: "how".to_string(),
        overline: "How it works".to_string(),
        heading: "Our process is designed with speed in mind".to_string(),
        description: Some(
            "Create an account, answer some questions, upload your documentation, and we will \
             turn around and file what you need expeditiously."
                .to_string(),
        ),
        items: vec![
            Step {
                title: "Create an account".to_string(),
                body: vec![vec![Run::plain(
                    "Set up your account so everything about your matter lives in one place.",
                )]],
            },
            Step {
                title: "Answer some questions".to_string(),
                body: vec![vec![Run::plain(
                    "A short questionnaire, scoped to what your filing actually needs.",
                )]],
            },
            Step {
                title: "Upload your documentation".to_string(),
                body: vec![vec![Run::plain(
                    "Add the documents your matter calls for; we tell you which ones.",
                )]],
            },
            Step {
                title: "We file what you need, expeditiously".to_string(),
                body: vec![vec![
                    Run::plain("A licensed attorney reviews it, then we file it and send you "),
                    Run::plain("the confirmation when it comes back."),
                ]],
            },
        ],
    }
}

/// The regulated claims on the firm's public pages.
///
/// `/navigator` and `/services` are the firm's, so the copy and the guards that
/// hold its claims in place live in the binary that publishes them rather than
/// in the application underneath.
#[cfg(test)]
mod firm_copy_tests {
    use webapp::foundation_marketing::{Band, Paragraph};

    /// Every word of prose a band renders, flattened. Titles, leads, overlines,
    /// descriptions, chips, and card bodies all count: a reader does not
    /// distinguish the struct field a claim arrived in.
    ///
    /// The `overline` and `description` fields are read for exactly that
    /// reason. They were previously skipped, which meant a regulated claim —
    /// a rate, a turnaround promise, a comparative superlative — placed in a
    /// band's description was invisible to every guard in this module while
    /// rendering to the reader like any other sentence. A guard that reads
    /// only some of the page is a guard that reports green on the half it
    /// cannot see.
    fn band_text(band: &Band) -> String {
        fn paragraphs(body: &[Paragraph]) -> String {
            body.iter()
                .flat_map(|p| p.iter().map(|r| r.text.clone()))
                .collect::<Vec<_>>()
                .join(" ")
        }
        match band {
            Band::Statement {
                heading,
                lead,
                body,
            } => format!("{heading} {lead} {}", paragraphs(body)),
            Band::Cards {
                overline,
                heading,
                description,
                items,
                ..
            } => {
                let cards = items
                    .iter()
                    .map(|c| format!("{} {} {}", c.title, c.chips.join(" "), paragraphs(&c.body)))
                    .collect::<Vec<_>>()
                    .join(" ");
                let description = description.clone().unwrap_or_default();
                format!("{overline} {heading} {description} {cards}")
            }
            Band::Steps {
                overline,
                heading,
                description,
                items,
                ..
            } => {
                let steps = items
                    .iter()
                    .map(|s| format!("{} {}", s.title, paragraphs(&s.body)))
                    .collect::<Vec<_>>()
                    .join(" ");
                let description = description.clone().unwrap_or_default();
                format!("{overline} {heading} {description} {steps}")
            }
            Band::Cta { heading, body, .. } => {
                format!("{heading} {}", body.clone().unwrap_or_default())
            }
        }
    }

    fn page_text(bands: &[Band]) -> String {
        bands.iter().map(band_text).collect::<Vec<_>>().join(" ")
    }

    /// The fee schedule's cards, resolved from the page rather than restated.
    ///
    /// Every guard below reads the rendered band, so adding a matter without
    /// scoping it — or shipping a placeholder in its price — fails here rather
    /// than passing against a list this file happened to keep in step.
    fn fee_cards(
        content: &webapp::foundation_marketing::PageContent,
    ) -> &[webapp::foundation_marketing::Card] {
        content
            .bands
            .iter()
            .find_map(|band| match band {
                Band::Cards { items, .. } => Some(items.as_slice()),
                _ => None,
            })
            .expect("the Legal Services page renders its fee schedule as a card band")
    }

    /// The platform page offers one concrete pro bono co-counsel invitation.
    #[test]
    fn the_navigator_page_invites_pro_bono_foundation_co_counsel() {
        let content = super::navigator();
        let text = format!("{} {}", page_text(&content.bands), content.meta_description);
        assert!(
            text.contains("Co-Counsel a Pro Bono Case with us and the Neon Law Foundation"),
            "the only invitation is pro bono Foundation co-counsel: {text}"
        );
        assert!(
            text.contains(
                "serving clients as expeditiously, precisely, accurately, and in alignment with their interests"
            ),
            "the client-serving purpose is stated outright: {text}"
        );
        assert!(
            !text.to_lowercase().contains("fractional"),
            "the retired fractional offer must not remain: {text}"
        );
        match content.bands.last() {
            Some(Band::Cta {
                email,
                email_subject,
                ..
            }) => {
                assert_eq!(email, views::brand::firm_email());
                assert_eq!(
                    email_subject.as_deref(),
                    Some("Co-Counseling for Good with AI")
                );
            }
            _ => panic!("the co-counsel invitation must be the page CTA"),
        }
    }

    /// The platform page is not a CTO/CISO or consulting advertisement.
    #[test]
    fn the_navigator_page_removes_the_cto_ciso_offer() {
        let content = super::navigator();
        let text = format!("{} {}", page_text(&content.bands), content.meta_description);
        assert!(
            !text.to_lowercase().contains("cto"),
            "no CTO offer reaches the page: {text}"
        );
        assert!(
            !text.to_lowercase().contains("ciso"),
            "no CISO offer reaches the page: {text}"
        );
        assert!(
            !text.contains("law-related service"),
            "the retired consulting characterization must not remain: {text}"
        );
        assert!(
            !text.contains("Bring a case") && !text.contains("See it in practice"),
            "the sales-style card grid must not remain: {text}"
        );
        assert!(
            !text.contains("Navigator is the AI system we build")
                && !text.contains("everyone loves vibe-coding"),
            "the retired explanatory copy must not remain: {text}"
        );
    }

    /// The Legal Services page is a schedule of scoped matters.
    ///
    /// This is the shape the fee schedule will be published in, asserted before
    /// the figures land. It replaced a page held to the opposite rule — guarded
    /// against containing a `$` at all, because the firm quoted every
    /// engagement privately — so what matters here is that the structure
    /// survives: a list of named matters, each with the scope its future fee
    /// will buy. A card that lost its scope line would leave a bare price with
    /// no boundary the moment a number arrived beside it.
    #[test]
    fn the_schedule_lists_scoped_matters() {
        let content = super::legal_services();
        let fees = fee_cards(&content);
        assert!(
            fees.len() >= 5,
            "the schedule is the page; {} matters is not a schedule",
            fees.len()
        );
        for card in fees {
            assert!(
                !card.body.is_empty(),
                "{} names no scope, which reads as covering everything",
                card.title
            );
        }
    }

    /// A fee is either published properly or not published at all.
    ///
    /// Every entry is unset today and the firm sets them when it decides them,
    /// so this guards the transition rather than the current state: whatever
    /// appears in that column has to be a real figure. A blank string, a `TBD`,
    /// or a `—` would render as a price tag the reader cannot parse, and a
    /// placeholder shipped by accident is exactly the failure that guard is
    /// for.
    #[test]
    fn any_published_fee_is_a_real_figure() {
        let content = super::legal_services();
        for card in fee_cards(&content) {
            let Some(price) = card.chips.first() else {
                continue;
            };
            assert!(
                price.starts_with('$'),
                "{} publishes {price:?}, which is not a fee",
                card.title
            );
            assert!(
                price.chars().any(|c| c.is_ascii_digit()),
                "{} publishes {price:?}, which carries no amount",
                card.title
            );
        }
        assert!(
            fee_cards(&content).iter().all(|card| card.chips.len() <= 1),
            "a matter carries one fee or none; two prices on one card is not a flat fee"
        );
    }

    /// A fee that depends on a government body's own charge says so.
    ///
    /// The firm cannot control what the Secretary of State, the IRS, or the
    /// USPTO charges, and those change without asking us. A formation priced at
    /// a bare `$700` would be read as the whole cost of forming a company, and
    /// the state's invoice afterwards would land as a surprise charge from a
    /// firm that advertised a flat fee.
    #[test]
    fn a_fee_with_a_pass_through_names_it() {
        let content = super::legal_services();
        for card in fee_cards(&content) {
            let Some(price) = card.chips.first() else {
                continue;
            };
            if price.contains('+') {
                assert!(
                    price.contains("fee"),
                    "{} adds a pass-through without naming it: {price}",
                    card.title
                );
            }
        }
    }

    /// Every matter whose fee depends on a government charge says so in its
    /// scope, whether or not a figure is set yet.
    ///
    /// The pass-through is a property of the work, not of the price, so it can
    /// be stated before the fee is. A reader deciding whether they can afford a
    /// formation needs to know a second bill is coming even on a page that has
    /// not named the first one.
    #[test]
    fn a_matter_with_a_government_charge_discloses_it() {
        let content = super::legal_services();
        let cards = fee_cards(&content);
        for matter in ["LLC formation", "Trademark application"] {
            let card = cards
                .iter()
                .find(|card| card.title == matter)
                .unwrap_or_else(|| panic!("{matter} is on the schedule"));
            let scope: String = card
                .body
                .iter()
                .flat_map(|p| p.iter().map(|r| r.text.clone()))
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                scope.contains("fee"),
                "{matter} carries a government charge the scope must disclose: {scope}"
            );
        }
    }

    /// The page states the attorney review the work rests on.
    ///
    /// A priced list of legal documents is the shape a document mill takes, and
    /// the one thing separating this page from one is that a licensed attorney
    /// reads what goes out. That has to be on the page, not only in the footer.
    #[test]
    fn the_legal_services_page_names_attorney_review() {
        let content = super::legal_services();
        let text = format!(
            "{} {} {} {}",
            content.title,
            content.tagline,
            content.meta_description,
            page_text(&content.bands)
        );
        assert!(
            text.to_lowercase().contains("attorney"),
            "the page states the attorney review the work rests on: {text}"
        );
    }

    /// The two quoted practices publish no figure.
    ///
    /// Litigation and fractional GC are quoted per engagement because their
    /// scope is not knowable in advance. The consumer schedule does not license
    /// a number on those pages: a published litigation "price" would be a floor
    /// dressed as a fee, which is the misleading-fee-advertising problem the
    /// flat-fee schedule exists to avoid.
    #[test]
    fn the_services_page_does_not_price_litigation_or_fractional_gc() {
        let content = super::legal_services();
        let fees = fee_cards(&content);
        for quoted in ["litigation", "fractional"] {
            assert!(
                !fees
                    .iter()
                    .any(|card| card.title.to_lowercase().contains(quoted)),
                "{quoted} is quoted per engagement and must not appear in the fee schedule"
            );
        }
    }
}
