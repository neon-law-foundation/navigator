//! The firm's public Dioxus SSR pages, and the content each one renders.
//!
//! Every firm page renders through the Dioxus port, so this module — not an
//! Axum route table — is where the firm's public surface actually lives. The
//! content resolvers read the mounted brand bundle directly rather than the
//! ambient branding, because a page's copy is baked at router-build time,
//! before any request scopes branding.

use portal::hosting::PublicRouter as Router;
use portal::{dioxus_app, secure_cookies, AppState, WorkshopIndex};

use crate::firm_copy;

const WORKSHOP_INDEX_TITLE: &str = "Workshops";
const WORKSHOP_INDEX_LEDE: &str =
    "Workshops are our hands-on classes for lawyers and legal professionals who run Neon Law \
     Navigator. Each one is a working session against the real application.";
const WORKSHOP_INDEX_FOOTNOTE: &str = "More workshops land here as we run them.";

const PRESENTATION_INDEX_TITLE: &str = "Presentations";
const PRESENTATION_INDEX_LEDE: &str =
    "Presentations are the talks we give at meetups and conferences. Every code slide is an exact \
     copy of the shipped repository, kept honest by a test that fails the build when one drifts.";
const PRESENTATION_INDEX_FOOTNOTE: &str = "More talks land here as we give them.";

/// Build the `presentations` index: every material the manifest files under
/// that category, in manifest order.
///
/// The talks catalog is the firm's now, so the card list is Foundation-free and
/// the contact address is the firm's inbox — a reader on `neonlaw.com` who
/// wants us at their meetup writes to the firm, not to the nonprofit.
fn presentation_index_content(
    workshops: &WorkshopIndex,
) -> webapp::nebula_index::NebulaIndexContent {
    nebula_index_content(
        workshops,
        "presentations",
        PRESENTATION_INDEX_TITLE,
        PRESENTATION_INDEX_LEDE,
        PRESENTATION_INDEX_FOOTNOTE,
    )
}

/// Build the `workshops` index — the catalog page for the Navigator classes.
///
/// Gated exactly like the classes it lists. The page names the lawyer
/// workbench, the admin deployment tier, and the contribution loop, so a
/// reader who cannot open a single class gains nothing from the list.
fn workshop_index_content(workshops: &WorkshopIndex) -> webapp::nebula_index::NebulaIndexContent {
    nebula_index_content(
        workshops,
        "workshops",
        WORKSHOP_INDEX_TITLE,
        WORKSHOP_INDEX_LEDE,
        WORKSHOP_INDEX_FOOTNOTE,
    )
}

/// One category index's Dioxus content: every material the manifest files
/// under `category`, in manifest order.
///
/// The contact address is the firm's on both catalogs — the firm gives the
/// talks and runs the classes, so a reader who wants either writes to it.
fn nebula_index_content(
    workshops: &WorkshopIndex,
    category: &str,
    title: &str,
    lede: &str,
    footnote: &str,
) -> webapp::nebula_index::NebulaIndexContent {
    webapp::nebula_index::NebulaIndexContent {
        title: title.to_string(),
        lede: lede.to_string(),
        materials: workshops
            .materials()
            .iter()
            .filter(|m| m.category == category)
            .map(|m| webapp::nebula_index::NebulaMaterial {
                href: format!("/{}/{}", m.category, m.slug),
                eyebrow: m.audience.clone(),
                title: m.title.clone(),
                summary: m.benefit.clone(),
            })
            .collect(),
        contact_email: views::brand::firm_email().to_string(),
        footnote: footnote.to_string(),
    }
}

/// The firm host's public Dioxus SSR pages, as raw routers for
/// [`portal::bootstrap`]'s `host_dioxus` argument. The `neon` binary passes
/// these alongside the Foundation's, since one binary serves both faces.
/// `bootstrap` wraps each in the anonymous-access session boundary and the
/// shared layer stack, exactly as it does the built-in Dioxus routes.
///
/// Takes `state` because the content-backed pages (e.g. `/blog`) read request
/// state (`BlogIndex`) the router injects into the render context; the
/// brand-only pages ignore it.
#[must_use]
#[allow(clippy::too_many_lines)] // A flat list of the firm's public page routers.
pub fn firm_public_dioxus_routers(state: &AppState) -> Vec<Router> {
    // The blog index is per-host static content; build its wasm-safe post list
    // once (with the shared date formatting) for the Dioxus router to inject.
    let blog_posts = webapp::blog_index::BlogPosts(
        state
            .blog
            .posts()
            .iter()
            .map(|post| webapp::blog_index::BlogPostSummary {
                slug: post.slug.clone(),
                date: format_blog_date(post.date),
                title: post.title.clone(),
                description: post.description.clone(),
            })
            .collect(),
    );
    // The full post bodies keyed by slug — the `/blog/{slug}` route's pre-layer
    // resolves the matched post from this set (or redirects / 404s).
    let blog_post_set = webapp::blog_post::BlogPostSet(std::sync::Arc::new(
        state
            .blog
            .posts()
            .iter()
            .map(|post| {
                (
                    post.slug.clone(),
                    webapp::blog_post::BlogPostContent {
                        date: format_blog_date(post.date),
                        title: post.title.clone(),
                        body_html: post.body_html.clone(),
                    },
                )
            })
            .collect(),
    ));
    let mut routers = vec![
        dioxus_app::blog_index_router(blog_posts),
        dioxus_app::blog_post_router(blog_post_set),
    ];
    // The firm `/contact` page, content resolved from the
    // mounted brand bundle. Resolve the branding from `state.brand_bundle`
    // (mirroring `bootstrap`) rather than the
    // ambient `current()`: this content is baked at router-build time, before
    // any request scopes branding, so a white-label deploy's contact addresses
    // must come from the bundle directly and not the process/default fallback.
    let contact_branding = state
        .brand_bundle
        .as_ref()
        .map_or(&views::brand::DEFAULT_BRANDING, |bundle| {
            views::brand::Branding::from_manifest(&bundle.manifest)
        });
    routers.push(dioxus_app::contact_router(
        "/contact",
        resolve_firm_contact_content(contact_branding),
    ));
    // The home page (`/`): a static statement of the practice, no per-request
    // data.
    routers.push(dioxus_app::home_router(
        "/",
        resolve_firm_home_content(contact_branding),
    ));
    // The practice pages the home page's cards lead into. Static copy like the
    // home page's, resolved here so the `<title>` names the mounted brand.
    routers.push(dioxus_app::litigation_router(
        "/litigation",
        resolve_litigation_content(contact_branding),
    ));
    routers.push(dioxus_app::transactional_router(
        "/fractional-gc",
        resolve_transactional_content(contact_branding),
    ));
    // The platform page. It sits on the firm's host rather than the
    // Foundation's because it carries a commercial offer — the nonprofit may
    // disclose who built its software, but it may not advertise that firm's
    // consulting practice.
    routers.push(dioxus_app::firm_marketing_page_router(
        dioxus_app::FIRM_NAVIGATOR_PATH,
        firm_copy::navigator(),
    ));
    // The Legal Services page. Like the platform page above it is a marketing
    // page, not a `/services/*` catalog entry: one page describing the routine,
    // one-time work, quoted through `/contact` and publishing no price. It is
    // where the firm's government-form filing work lives.
    routers.push(dioxus_app::firm_marketing_page_router(
        dioxus_app::FIRM_SERVICES_PATH,
        firm_copy::legal_services(),
    ));
    // The talks catalog, and the five read routes each talk publishes: the
    // hub, its light table, the classroom step face, the projector face a
    // presenter opens on a second screen, and the certificate confirmation.
    // The hub's pre-layer also owns the `…/{slug}.md` raw-Markdown twin, which
    // matchit routes there rather than to a second path.
    //
    // Mounted with `NebulaChrome::Firm` so a talk wears the firm's header and
    // its regulated footer. The gated workshops catalog and classes use the
    // same host and chrome, behind their session and policy boundaries.
    routers.push(dioxus_app::nebula_index_router(
        dioxus_app::PRESENTATION_INDEX_PATH,
        presentation_index_content(&state.workshops),
        dioxus_app::NebulaChrome::Firm,
    ));
    routers.extend(dioxus_app::nebula_material_routers(
        &dioxus_app::PRESENTATION_PATHS,
        state.workshops.clone(),
        &state.sessions,
        secure_cookies(state),
        dioxus_app::NebulaChrome::Firm,
    ));
    // The Navigator classes, anonymous like the talks.
    //
    // They were firm-internal training behind the session boundary and the
    // embedded policy, readable by Clerk, Lawyer, Admin, and Owner alone. The
    // repository is open source now, and the classes teach the software it
    // publishes — gating them would put a login door in front of the one
    // document that explains how to run what anyone can already clone.
    //
    // The certificate `POST` keeps its own gate: who may CLAIM a completion
    // certificate is an authorization question, and it stays one even when the
    // material is free to read.
    routers.push(dioxus_app::nebula_index_router(
        dioxus_app::WORKSHOP_INDEX_PATH,
        workshop_index_content(&state.workshops),
        dioxus_app::NebulaChrome::Firm,
    ));
    routers.extend(dioxus_app::nebula_material_routers(
        &dioxus_app::WORKSHOP_PATHS,
        state.workshops.clone(),
        &state.sessions,
        secure_cookies(state),
        dioxus_app::NebulaChrome::Firm,
    ));
    routers.push(portal::nebula_workshop_command_routes(state));
    routers
}

/// Human-readable publish date for the blog (e.g. `"June 19, 2026"`).
/// Kept in `web` so the `views` crate stays free of `chrono`.
fn format_blog_date(date: chrono::NaiveDate) -> String {
    date.format("%B %-d, %Y").to_string()
}

/// Resolve the firm `/contact` content from the mounted `branding`'s addresses
/// — the wasm-safe [`webapp::contact_page::ContactContent`] the Dioxus contact
/// router injects. Takes the resolved `branding` explicitly because the content
/// is baked at router-build time, before per-request branding scope.
fn resolve_firm_contact_content(
    branding: &views::brand::Branding,
) -> webapp::contact_page::ContactContent {
    let firm_name = branding.firm.site_name;

    let page_title = "Contact";
    webapp::contact_page::ContactContent {
        head_title: format!("{firm_name} | {page_title}"),
        meta_description: format!(
            "Reach {firm_name} for estate planning, corporate formation, litigation, and ongoing \
             legal services."
        ),
        page_title: page_title.to_string(),
        firm_heading: firm_name.to_string(),
        // No figure here. No page on this host posts a rate — every engagement
        // is quoted through this page — so a consultation fee would be the first
        // posted number on a surface whose whole purpose is to start a
        // conversation before anything is priced. The page promises the quote,
        // not its amount.
        firm_intro: format!(
            "Email {firm_name} with a short description of the matter — estate planning, \
             corporate formation, ongoing services. We respond within one business day with a \
             flat-fee quote and a calendar link. The first appointment is 30 minutes with a \
             licensed attorney."
        ),
        email_label: "Email".to_string(),
        phone_label: "Phone".to_string(),
        firm_email: branding.firm_email.to_string(),
        firm_phone: branding.firm_phone.to_string(),
    }
}

/// The litigation areas the firm takes, in the order the public site lists
/// them. Areas of practice, not a priced catalog: nothing here is orderable,
/// and every fee is still quoted through `/contact`.
const LITIGATION_AREAS: &[&str] = &[
    "Deceptive business practices",
    "Cybersecurity",
    "E-Privacy",
    "Unauthorized transfers",
    "Unfair competition",
    "Crypto",
    "AI",
    "Defamation",
    "Trade secrets & trademarks",
    "Contract disputes",
    "Complex commercial litigation",
    "Technology product liability",
];

/// The areas the firm's fractional general counsel and transactional practice
/// covers, in the order the public site lists them.
const TRANSACTIONAL_AREAS: &[&str] = &[
    "Contracts",
    "Licenses",
    "Financings",
    "General corporate advice",
];

/// The areas the firm's regulatory, investigations, and crisis-response
/// practice covers, in the order the public site lists them.
const REGULATORY_AREAS: &[&str] = &[
    "AI",
    "Data",
    "National security",
    "Privacy",
    "Security incident response",
    "Sensitive crisis investigations",
    "Copyright",
    "Right of publicity",
    "Anti-circumvention",
];

/// The routine, one-time work the firm's Legal Services page covers, in the
/// order the public site lists them. Areas of practice, not a priced catalog
/// and not named products.
const LEGAL_SERVICES_AREAS: &[&str] = &[
    "Business formation",
    "Nonprofit formation",
    "Trademarks",
    "Licensing",
    "Mutual NDAs",
    "Wills & trusts",
];

/// One plain run of practice prose.
fn plain(text: &str) -> webapp::home::CopyRun {
    webapp::home::CopyRun {
        text: text.to_string(),
        emphasis: false,
    }
}

/// Build the litigation header the home page leads its practice section with.
fn resolve_litigation_header() -> webapp::home::LitigationHeader {
    webapp::home::LitigationHeader {
        eyebrow: "The practice".to_string(),
        heading: "Litigation".to_string(),
        areas: LITIGATION_AREAS
            .iter()
            .map(|area| (*area).to_string())
            .collect(),
        // Body copy carries no emphasis runs. Bold inside a paragraph pulls the
        // eye to a phrase the sentence had already earned, and three cards each
        // bolding their own two phrases read as a page shouting in six places.
        body: vec![
            vec![plain(
                "We represent founders, emerging companies, consumers, and investors in \
                 high-stakes disputes, with active matters in state and federal courts, as well \
                 as arbitration, in California, D.C., New York, and elsewhere.",
            )],
            vec![plain(
                "We are comfortable on both sides of the v: Plaintiff and Defense.",
            )],
        ],
    }
}

/// Build the two practices the home page lists under the litigation header:
/// fractional general counsel — the company-counsel and transactional work with
/// the regulatory, investigations, and crisis-response counseling folded into
/// it — and Legal Services, the routine one-time work. Areas of practice like
/// the litigation card's, so neither publishes a price or names a product.
fn resolve_practice_cards() -> Vec<webapp::home::PracticeCard> {
    use webapp::home::{PracticeCard, PracticeMark};

    vec![
        PracticeCard {
            mark: PracticeMark::Globe,
            heading: "Fractional general counsel".to_string(),
            areas: TRANSACTIONAL_AREAS
                .iter()
                .chain(REGULATORY_AREAS.iter())
                .map(|area| (*area).to_string())
                .collect(),
            body: vec![
                vec![plain(
                    "We serve as fractional outside general counsel for multiple fast-growing \
                     companies, from innovative AI solutions to cybersecurity platforms to \
                     consumer products. We partner with our clients and manage their legal \
                     roadmap in a way that enables them to focus on growing their business.",
                )],
                vec![plain(
                    "We also counsel clients on AI and emerging technology regulation across \
                     global jurisdictions, adapting policies and risk to agentic workflows.",
                )],
            ],
        },
        PracticeCard {
            mark: PracticeMark::Shield,
            heading: "Legal Services".to_string(),
            areas: LEGAL_SERVICES_AREAS
                .iter()
                .map(|area| (*area).to_string())
                .collect(),
            body: vec![
                vec![plain(
                    "For clients who are not engaged with us on litigation or fractional GC \
                     projects, we offer one-time legal work such as forming a company, planning \
                     an estate, or other routine matters.",
                )],
                vec![plain(
                    "Our process is designed with speed in mind. Create an account, answer some \
                     questions, upload your documentation, and we will turn around and file what \
                     you need expeditiously.",
                )],
            ],
        },
    ]
}

/// Resolve the firm home page's static copy from the mounted `branding` — the
/// wasm-safe [`webapp::home::HomeContent`] the Dioxus home router injects.
/// Brand-safe like [`resolve_firm_contact_content`]: the `<title>` names the
/// mounted brand, resolved at router-build time. The body is the hero the
/// page opens on, then the practice statement over the practice cards — the
/// litigation header, then fractional general counsel and Legal Services under
/// it. Areas of practice and a record, not a priced service catalog.
pub(crate) fn resolve_firm_home_content(
    branding: &views::brand::Branding,
) -> webapp::home::HomeContent {
    let mark = branding.firm.site_name;
    webapp::home::HomeContent {
        head_title: format!("{mark} | {}", "Home"),
        meta_description: "A boutique law firm for high-stakes disputes and emerging technology \
                           companies — litigation on both sides of the v., and company counsel on \
                           a flat monthly fee."
            .to_string(),
        // No hero photograph. The page opens on the practice statement itself:
        // a consumer deciding whether they can afford a lawyer is served by the
        // first sentence, not by a landscape above it.
        hero: None,
        // One line. The hero is the first thing under the wordmark and it has
        // to be read at a glance; the list of who the firm serves and what it
        // protects them from is what the practice cards below are for.
        heading: "A boutique law firm for high-stakes disputes and emerging technology companies"
            .to_string(),
        lead: "We represent clients in litigation and handle transactional work on a flat fee, \
               monthly, or contingency fee basis. We enjoy working with our clients to design \
               case-specific arrangements to align our incentives as much as possible with our \
               clients' successes."
            .to_string(),
        contact_href: "/contact".to_string(),
        contact_label: "Contact us".to_string(),
        litigation: resolve_litigation_header(),
        practices: resolve_practice_cards(),
    }
}

/// Split a heading into its words, marking the first `accent_words` of them as
/// the run the firm sets in its own colour.
fn hero_words(heading: &str, accent_words: usize) -> Vec<webapp::litigation_page::HeroWord> {
    heading
        .split_whitespace()
        .enumerate()
        .map(|(index, text)| webapp::litigation_page::HeroWord {
            text: text.to_string(),
            accent: index < accent_words,
        })
        .collect()
}

/// Resolve the firm `/litigation` page — the statement, the practice in two
/// paragraphs, and the disclaimer.
///
/// Brand-safe like [`resolve_firm_home_content`]: the `<title>` names the
/// mounted brand, resolved at router-build time. Nothing here describes a matter
/// the firm handled, which is what lets the page carry a record-free
/// past-results disclaimer honestly.
///
/// **The body is the firm's own filed copy and this resolver holds it verbatim.**
/// The page arrived at these two paragraphs by subtraction: it was a Rule 23
/// explainer with six certification-element cards, an authority strip, a phase
/// rail, a chip list, and a fee section. Each was a reasonable answer to a
/// question a prospective client does not walk in with. What the firm wanted to
/// say fits in two paragraphs, so that is the page.
///
/// Those paragraphs name fee arrangements — contingency, monthly, and "no cost
/// due if we lose" — which is why the no-fee-copy guard the earlier revision
/// added is gone. For this practice the arrangement is part of the offer rather
/// than a term to settle later: a reader deciding whether to call needs to know
/// that a contingency case costs them nothing to bring. Fee *amounts* stay off
/// the page, and the currency guard still holds that.
pub(crate) fn resolve_litigation_content(
    branding: &views::brand::Branding,
) -> webapp::litigation_page::LitigationContent {
    use webapp::litigation_page::LitigationContent;

    let mark = branding.firm.site_name;
    LitigationContent {
        head_title: format!("{mark} | Litigation"),
        meta_description: "Litigation on both sides of the v. — plaintiff and defense. Complex \
                           technology disputes for companies, and fraud cases for the people on \
                           the receiving end."
            .to_string(),
        eyebrow: "Litigation — plaintiff and defense".to_string(),
        heading: hero_words("Zealous advocates on both sides of the v.", 2),
        lead: "We try cases for the party bringing the claim and for the party answering it. The \
               work is the same discipline from either chair, and we take the side we are the \
               right lawyers for."
            .to_string(),
        cta_href: "/contact".to_string(),
        cta_label: "Contact us".to_string(),
        // The practice in the firm's own two paragraphs, as filed. The company
        // side first, then the individuals — and the fee arrangement in each,
        // because for this practice the arrangement *is* part of the offer: a
        // reader deciding whether to call needs to know a contingency case
        // costs them nothing to bring.
        body: vec![
            "We represent emerging companies, founders, and investors in complex disputes \
             involving cutting-edge technology. This includes disputes about cybersecurity, \
             investment disputes, business divorce, trademarks, trade secrets. We prefer not to \
             charge hourly for these cases, instead crafting contingency or monthly fee deals to \
             align our incentives with those of our clients."
                .to_string(),
            "We also represent individuals who have been defrauded by powerful corporations. This \
             includes victims of deceptive business practices, cyber security failures, \
             unauthorized cryptocurrency transfers, electronic privacy violations, and other \
             harmful business practices. These cases can be organized as individual disputes, \
             class actions, mass actions, or public entity representations (on behalf of cities, \
             counties, Native American tribes, etc.). We pursue nearly all of these cases at no \
             cost to our clients, taking our fees \u{201c}on contingency,\u{201d} as a percentage of the \
             total recovery (with no cost due if we lose)."
                .to_string(),
        ],
        disclaimer: "Prior results do not guarantee a similar outcome; every matter turns on its \
                     own facts. This page is attorney advertising and general information, not \
                     legal advice, and reading it creates no attorney-client relationship."
            .to_string(),
    }
}

/// What the flat monthly transactional fee covers.
const TRANSACTIONAL_INCLUDED: &[(&str, &str)] = &[
    (
        "Cap table management",
        "The ledger stays current as options are granted, exercised, and forfeited — reconciled \
         against the signed instruments and the board consents, not against the spreadsheet.",
    ),
    (
        "Employee and contractor agreements",
        "Offer letters, IP assignment, confidentiality, contractor agreements, and the \
         option-grant paperwork that has to match what the board actually approved.",
    ),
    (
        "Basic taxes and state filings",
        "Nevada Commerce Tax and Modified Business Tax filings, the annual list, and the \
         registered-agent and state calendar that keeps the entity in good standing.",
    ),
    (
        "Corporate housekeeping",
        "Board and stockholder consents, the minute book, and the corporate record a diligence \
         request is going to ask for on two days' notice.",
    ),
    (
        "Counsel on call",
        "The questions that would otherwise wait for a scheduled call. Ask them the day you have \
         them; the answer is inside the monthly fee.",
    ),
];

/// The customer's sales cycle, and the legal step that runs inside each stage
/// rather than after it.
const SALES_CYCLE: &[(&str, &str)] = &[
    (
        "Discovery call",
        "Mutual NDA out the same business day, from your paper or ours.",
    ),
    (
        "Evaluation",
        "Pilot or trial agreement, with the security and data terms your buyer's review is about \
         to ask for already in it.",
    ),
    (
        "Negotiation",
        "Standard MSA drafted or redlined in one business day; counterparty paper reviewed \
         against the fallback positions we set with you in advance.",
    ),
    (
        "Close",
        "Order form, signature routing, and the executed set filed where diligence will look for \
         it a year from now.",
    ),
];

/// Accurate, Efficient, Speedy — the three the practice is named by, each with
/// the sentence that turns it from an adjective into something checkable.
///
/// The bodies are the whole point of the section: "speedy" alone is hyperbole,
/// "one business day on a redline" is a commitment.
fn transactional_virtues() -> Vec<webapp::transactional_page::Virtue> {
    use webapp::transactional_page::Virtue;

    vec![
        Virtue {
            word: "Accurate".to_string(),
            body: "A licensed attorney reads and signs off on every document. The cap table \
                   reconciles to the signed instruments, and the agreements say what the deal \
                   actually is."
                .to_string(),
        },
        Virtue {
            word: "Efficient".to_string(),
            body: "One flat monthly fee covers the recurring work, so a routine question costs \
                   nothing to ask and the answer arrives the day you asked it."
                .to_string(),
        },
        Virtue {
            word: "Speedy".to_string(),
            body: "Turnarounds are published rather than negotiated deal by deal: one business \
                   day on a redline, same business day on an NDA."
                .to_string(),
        },
    ]
}

/// The included line items and the sales-cycle stages, mapped out of their
/// tables. Split out for the same reason as [`litigation_phases`].
fn transactional_sections() -> (
    Vec<webapp::transactional_page::Included>,
    Vec<webapp::transactional_page::SalesStage>,
) {
    use webapp::transactional_page::{Included, SalesStage};

    let included = TRANSACTIONAL_INCLUDED
        .iter()
        .map(|(name, body)| Included {
            name: (*name).to_string(),
            body: (*body).to_string(),
        })
        .collect();
    let cycle = SALES_CYCLE
        .iter()
        .map(|(stage, legal_step)| SalesStage {
            stage: (*stage).to_string(),
            legal_step: (*legal_step).to_string(),
        })
        .collect();
    (included, cycle)
}

/// The work quoted outside the monthly retainer.
///
/// Litigation carries a link to its own page; financings do not, because the
/// firm publishes no financings page and a link that went nowhere would be
/// worse than the sentence alone.
fn transactional_separate_work() -> Vec<webapp::transactional_page::SeparateWork> {
    use webapp::transactional_page::SeparateWork;

    vec![
        SeparateWork {
            name: "Financings".to_string(),
            body: "Priced rounds, SAFEs, convertible notes, and the closing set that goes with \
                   them. Quoted per round once we have seen the term sheet."
                .to_string(),
            href: None,
            link_label: None,
        },
        SeparateWork {
            name: "Litigation".to_string(),
            body: "Disputes, demands, and class actions, on either side of the caption. Quoted \
                   per phase after a case assessment."
                .to_string(),
            href: Some("/litigation".to_string()),
            link_label: Some("The litigation practice".to_string()),
        },
    ]
}

/// Resolve the firm `/fractional-gc` page — the flat-monthly-fee company
/// counsel practice, the published turnaround, and the work that sits outside
/// the retainer.
///
/// Brand-safe like [`resolve_firm_home_content`]. The page names how the flat
/// monthly fee works and sends the figure itself to `/contact`; it publishes no
/// amount.
pub(crate) fn resolve_transactional_content(
    branding: &views::brand::Branding,
) -> webapp::transactional_page::TransactionalContent {
    use webapp::transactional_page::TransactionalContent;

    let mark = branding.firm.site_name;
    let (included, cycle) = transactional_sections();
    TransactionalContent {
        head_title: format!("{mark} | Fractional General Counsel"),
        meta_description: "Company counsel on a flat monthly fee — cap table, employee \
                           agreements, and state tax filings, with a one-business-day redline \
                           turnaround."
            .to_string(),
        eyebrow: "Fractional General Counsel".to_string(),
        heading: "Accurate. Efficient. Speedy.".to_string(),
        lead: "Company counsel on one flat monthly fee, working at the pace your sales cycle \
               already runs at. A redline comes back in one business day."
            .to_string(),
        cta_href: "/contact".to_string(),
        cta_label: "Contact us".to_string(),
        virtues: transactional_virtues(),
        msa_term: "MSA — master services agreement".to_string(),
        msa_definition: "The one contract that sets the terms between you and a customer once: \
                         payment, liability, IP ownership, confidentiality, term, and \
                         termination. Every later deal becomes a short order form that points \
                         back at it instead of a fresh negotiation, which is why getting the MSA \
                         right is what makes the next ten deals quick."
            .to_string(),
        fee_heading: "One flat monthly fee".to_string(),
        fee_body: "One amount, the same in a quiet month as in a loud one, billed monthly. We \
                   quote it for your company rather than posting a number here, and it is fixed \
                   in the engagement letter before the first month runs."
            .to_string(),
        included_heading: "What the monthly fee covers".to_string(),
        included,
        cycle_heading: "It runs inside your sales cycle".to_string(),
        cycle_body: "Legal is a step inside the stage your rep is already in, not a stage that \
                     comes after the deal. Published turnarounds are what make that forecastable."
            .to_string(),
        cycle,
        separate_heading: "Priced separately".to_string(),
        separate_body: "Two kinds of work sit outside the retainer, because neither is recurring \
                        and neither can be quoted by the month."
            .to_string(),
        separate: transactional_separate_work(),
    }
}
