//! Pre-matter conflict check — the graph traversal that runs *before* a
//! Project is created.
//!
//! # The shape of the problem
//!
//! A law firm may not open a matter that is adverse to, or improperly
//! entangled with, a client it already serves (Model Rules 1.7 / 1.9,
//! imputed firm-wide by 1.10). Answering "would this new matter
//! conflict?" is a **graph reachability** question: start from the
//! proposed client and the proposed entity, walk the relationships, and
//! see whether you arrive at another party the firm already represents —
//! especially across an `adverse_to` edge.
//!
//! # Why petgraph over Postgres, an extension, or Neo4j
//!
//! Postgres stays the source of truth. Cloud SQL forbids custom
//! extensions, so Apache AGE is out; a small firm's whole relationship
//! graph is a few thousand edges, so a separate Neo4j (a second source
//! of truth to keep in sync) is unjustified. Instead [`build_graph`]
//! loads the relevant rows into an in-memory [`petgraph`] graph per
//! check and traverses that. The graph is a *transient view* of the
//! ledger, never a store. When scale eventually demands a real graph
//! engine, it swaps in behind this module's functions — callers don't
//! change.
//!
//! # What feeds the graph
//!
//! - `person_entity_roles` — structural ties (a person manages / owns /
//!   is a member of an entity). Always present, always confidence 100.
//! - `relationship_edges` — the supplemental typed edges this feature
//!   adds: adversity, related-party ties, and (later) edges an LLM
//!   parsed out of `relationship_logs.detail`. Each carries its own
//!   confidence and provenance.
//!
//! Findings are **advisory to clear, authoritative to block**: a
//! confident, direct `adverse_to` link to an existing client is a hard
//! block; everything else is surfaced for a human to adjudicate (the
//! firm's standing `@cleared` discipline). The graph can *raise* a
//! conflict; only a person can *clear* one — because the graph is never
//! known to be complete.

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::graph::{NodeIndex, UnGraph};
use petgraph::visit::EdgeRef;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entity::{
    disclosure, entity as entities, person, person_entity_role, project, relationship_edge,
};
use crate::Db;

/// How many relationship hops out from the proposed matter the check
/// explores. Three reaches "my counterparty's affiliate's owner" — deep
/// enough for imputation without drowning staff in distant noise.
const MAX_HOPS: usize = 3;

/// A path weaker than this (after multiplying edge confidences) is
/// dropped — a chain of low-confidence guesses should not raise a
/// finding on its own.
const REVIEW_FLOOR_PCT: i32 = 25;

/// A `Block` requires at least this much confidence. Below it, even an
/// adverse link is downgraded to `Review` rather than hard-stopping the
/// open on a shaky edge.
const BLOCK_FLOOR_PCT: i32 = 80;

/// A `Block` requires the adverse counterparty to be this close — a
/// direct or one-removed adversity. Distant adversity is a review item.
const BLOCK_MAX_HOPS: usize = 2;

/// The kind of row an edge endpoint points at.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NodeKind {
    Person,
    Entity,
}

impl NodeKind {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            relationship_edge::NODE_PERSON => Some(Self::Person),
            relationship_edge::NODE_ENTITY => Some(Self::Entity),
            _ => None,
        }
    }
}

/// A node in the conflict graph: a typed reference to one `persons` or
/// `entities` row.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeRef {
    pub kind: NodeKind,
    pub id: Uuid,
}

impl NodeRef {
    /// A `persons` node.
    #[must_use]
    pub fn person(id: Uuid) -> Self {
        Self {
            kind: NodeKind::Person,
            id,
        }
    }

    /// An `entities` node.
    #[must_use]
    pub fn entity(id: Uuid) -> Self {
        Self {
            kind: NodeKind::Entity,
            id,
        }
    }
}

/// How serious a finding is. `Block` hard-stops the automated open;
/// `Review` surfaces the finding but lets authorized staff proceed after
/// acknowledging it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Review,
    Block,
}

/// Why a finding fired — the legal shape of the concern.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    /// The path crosses an `adverse_to` edge to a party the firm serves.
    Adverse,
    /// The proposed parties share an entity / party with another matter
    /// the firm already runs for a different client.
    SharedParty,
    /// A recorded `disclosures` row (conflict / related-party) touches a
    /// node in the proposed matter's neighborhood.
    Disclosure,
}

/// One conflict the check surfaced, with enough context for staff to
/// adjudicate it rather than trust it blindly.
#[derive(Clone, Debug)]
pub struct ConflictFinding {
    pub severity: Severity,
    pub reason: Reason,
    /// Human label of the party the proposed matter collides with.
    pub counterparty: String,
    /// Full sentence including the relationship path that produced it.
    pub explanation: String,
    /// Confidence the path is real, 0–100.
    pub confidence_pct: i32,
}

/// The result of a pre-matter conflict check.
#[derive(Clone, Debug, Default)]
pub struct ConflictReport {
    pub findings: Vec<ConflictFinding>,
}

impl ConflictReport {
    /// No conflicts at all — the matter may open without staff review.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.findings.is_empty()
    }

    /// At least one `Block`-severity finding — the automated open is
    /// hard-stopped and cannot be overridden from the create form.
    #[must_use]
    pub fn has_blocking(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Block)
    }

    /// One human-readable line per finding, for the create form and for
    /// the `relationship_logs` audit entry when staff override a review.
    #[must_use]
    pub fn summary_lines(&self) -> Vec<String> {
        self.findings
            .iter()
            .map(|f| {
                let tag = match f.severity {
                    Severity::Block => "BLOCK",
                    Severity::Review => "REVIEW",
                };
                format!(
                    "[{tag}] {} ({}% confidence)",
                    f.explanation, f.confidence_pct
                )
            })
            .collect()
    }
}

/// An edge weight in the in-memory graph.
struct EdgeMeta {
    kind: String,
    confidence_pct: i32,
}

/// The loaded conflict graph plus the lookups a check needs. Built once
/// per check by [`build_graph`].
pub struct ConflictGraph {
    graph: UnGraph<NodeRef, EdgeMeta>,
    index: HashMap<NodeRef, NodeIndex>,
    person_names: HashMap<Uuid, String>,
    entity_names: HashMap<Uuid, String>,
    /// Entity → the distinct client DRIs of its non-archived projects.
    entity_clients: HashMap<Uuid, HashSet<Uuid>>,
    /// Persons who are the client DRI of some non-archived project.
    client_persons: HashSet<Uuid>,
    /// Entity → its conflict / related-party disclosure summaries.
    entity_disclosures: HashMap<Uuid, Vec<String>>,
}

/// Load every row the conflict check traverses into an in-memory graph.
///
/// One pass over `person_entity_roles` + `relationship_edges` builds the
/// edges; `projects` and `disclosures` build the "who does the firm
/// already serve" lookups a finding tests against. Names are loaded so
/// findings read in plain language.
///
/// # Errors
///
/// Returns any `sea_orm::DbErr` from the underlying queries.
pub async fn build_graph(db: &Db) -> Result<ConflictGraph, sea_orm::DbErr> {
    let mut graph: UnGraph<NodeRef, EdgeMeta> = UnGraph::new_undirected();
    let mut index: HashMap<NodeRef, NodeIndex> = HashMap::new();

    let node = |g: &mut UnGraph<NodeRef, EdgeMeta>,
                idx: &mut HashMap<NodeRef, NodeIndex>,
                n: NodeRef| { *idx.entry(n).or_insert_with(|| g.add_node(n)) };

    // Structural person↔entity ties — always full confidence.
    for role in person_entity_role::Entity::find().all(db).await? {
        let p = node(&mut graph, &mut index, NodeRef::person(role.person_id));
        let e = node(&mut graph, &mut index, NodeRef::entity(role.entity_id));
        graph.add_edge(
            p,
            e,
            EdgeMeta {
                kind: role.role,
                confidence_pct: 100,
            },
        );
    }

    // Supplemental typed edges (adversity, related-party, LLM-parsed).
    for edge in relationship_edge::Entity::find().all(db).await? {
        let (Some(from_kind), Some(to_kind)) = (
            NodeKind::from_str(&edge.from_type),
            NodeKind::from_str(&edge.to_type),
        ) else {
            // An edge with an unknown endpoint kind can't be placed in
            // the graph; skip it rather than guess.
            continue;
        };
        let from = node(
            &mut graph,
            &mut index,
            NodeRef {
                kind: from_kind,
                id: edge.from_id,
            },
        );
        let to = node(
            &mut graph,
            &mut index,
            NodeRef {
                kind: to_kind,
                id: edge.to_id,
            },
        );
        graph.add_edge(
            from,
            to,
            EdgeMeta {
                kind: edge.kind,
                confidence_pct: edge.confidence_pct.clamp(0, 100),
            },
        );
    }

    let person_names = person::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect();
    let entity_names = entities::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|e| (e.id, e.name))
        .collect::<HashMap<_, _>>();

    let mut entity_clients: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
    let mut client_persons: HashSet<Uuid> = HashSet::new();
    for proj in project::Entity::find()
        .filter(project::Column::Status.ne("archived"))
        .all(db)
        .await?
    {
        if let Some(client) = proj.client_dri_person_id {
            entity_clients
                .entry(proj.entity_id)
                .or_default()
                .insert(client);
            client_persons.insert(client);
        }
    }

    let mut entity_disclosures: HashMap<Uuid, Vec<String>> = HashMap::new();
    for d in disclosure::Entity::find().all(db).await? {
        if matches!(d.kind.as_str(), "conflict" | "related_party") {
            if let Some(eid) = d.entity_id {
                entity_disclosures.entry(eid).or_default().push(d.summary);
            }
        }
    }

    Ok(ConflictGraph {
        graph,
        index,
        person_names,
        entity_names,
        entity_clients,
        client_persons,
        entity_disclosures,
    })
}

/// One node the traversal reached, with how it got there.
struct Reached {
    confidence_pct: i32,
    hops: usize,
    adverse_on_path: bool,
    path: String,
}

impl ConflictGraph {
    fn label(&self, n: NodeRef) -> String {
        match n.kind {
            NodeKind::Person => self
                .person_names
                .get(&n.id)
                .cloned()
                .unwrap_or_else(|| format!("person {}", n.id)),
            NodeKind::Entity => self
                .entity_names
                .get(&n.id)
                .cloned()
                .unwrap_or_else(|| format!("entity {}", n.id)),
        }
    }

    /// Breadth-first walk from both anchors, keeping the shortest path to
    /// each reachable node along with the multiplied confidence and
    /// whether an `adverse_to` edge lay on the way.
    fn reach(&self, anchors: &[NodeRef]) -> HashMap<NodeRef, Reached> {
        let mut reached: HashMap<NodeRef, Reached> = HashMap::new();
        let mut visited: HashSet<NodeRef> = HashSet::new();
        let mut queue: VecDeque<(NodeRef, i32, usize, bool, String)> = VecDeque::new();

        for &a in anchors {
            if visited.insert(a) {
                reached.insert(
                    a,
                    Reached {
                        confidence_pct: 100,
                        hops: 0,
                        adverse_on_path: false,
                        path: self.label(a),
                    },
                );
                queue.push_back((a, 100, 0, false, self.label(a)));
            }
        }

        while let Some((cur, conf, hops, adverse, path)) = queue.pop_front() {
            if hops >= MAX_HOPS {
                continue;
            }
            let Some(&cur_ix) = self.index.get(&cur) else {
                continue;
            };
            for edge in self.graph.edges(cur_ix) {
                let next_ix = if edge.source() == cur_ix {
                    edge.target()
                } else {
                    edge.source()
                };
                let next = self.graph[next_ix];
                if visited.contains(&next) {
                    continue;
                }
                let meta = edge.weight();
                let next_conf = conf * meta.confidence_pct / 100;
                if next_conf < REVIEW_FLOOR_PCT {
                    continue;
                }
                let next_adverse = adverse || meta.kind == relationship_edge::KIND_ADVERSE_TO;
                let next_path = format!("{path} —{}→ {}", meta.kind, self.label(next));
                visited.insert(next);
                reached.insert(
                    next,
                    Reached {
                        confidence_pct: next_conf,
                        hops: hops + 1,
                        adverse_on_path: next_adverse,
                        path: next_path.clone(),
                    },
                );
                queue.push_back((next, next_conf, hops + 1, next_adverse, next_path));
            }
        }
        reached
    }

    /// Run the conflict check for opening a matter for `client_person_id`
    /// against `entity_id`. The anchors are those two nodes; the report
    /// names every distinct firm-served party the proposed matter is
    /// entangled with.
    #[must_use]
    pub fn check(&self, client_person_id: Uuid, entity_id: Uuid) -> ConflictReport {
        let anchor_person = NodeRef::person(client_person_id);
        let anchor_entity = NodeRef::entity(entity_id);
        let reached = self.reach(&[anchor_person, anchor_entity]);

        let mut findings = Vec::new();
        for (&n, r) in &reached {
            // Adversity / shared-party concerns attach to entity nodes the
            // firm already serves and to *other* client persons.
            let counterparty_client = match n.kind {
                NodeKind::Entity => self
                    .entity_clients
                    .get(&n.id)
                    .is_some_and(|clients| clients.iter().any(|c| *c != client_person_id)),
                NodeKind::Person => n.id != client_person_id && self.client_persons.contains(&n.id),
            };

            if counterparty_client {
                let (severity, reason) = if r.adverse_on_path {
                    let blocking = r.confidence_pct >= BLOCK_FLOOR_PCT && r.hops <= BLOCK_MAX_HOPS;
                    (
                        if blocking {
                            Severity::Block
                        } else {
                            Severity::Review
                        },
                        Reason::Adverse,
                    )
                } else {
                    (Severity::Review, Reason::SharedParty)
                };
                let lead = match reason {
                    Reason::Adverse => "Adverse to a current client",
                    _ => "Shares a party with a current client's matter",
                };
                findings.push(ConflictFinding {
                    severity,
                    reason,
                    counterparty: self.label(n),
                    explanation: format!("{lead}: {}", r.path),
                    confidence_pct: r.confidence_pct,
                });
            }

            // Recorded disclosures on any reached entity always surface.
            if n.kind == NodeKind::Entity {
                if let Some(summaries) = self.entity_disclosures.get(&n.id) {
                    for summary in summaries {
                        findings.push(ConflictFinding {
                            severity: Severity::Review,
                            reason: Reason::Disclosure,
                            counterparty: self.label(n),
                            explanation: format!(
                                "Disclosure on {}: {summary} (via {})",
                                self.label(n),
                                r.path
                            ),
                            confidence_pct: r.confidence_pct,
                        });
                    }
                }
            }
        }

        // Stable order: blocks first, then by descending confidence, so
        // the most serious finding leads the form and the audit log.
        findings.sort_by(|a, b| {
            b.severity
                .eq(&Severity::Block)
                .cmp(&a.severity.eq(&Severity::Block))
                .then(b.confidence_pct.cmp(&a.confidence_pct))
                .then(a.counterparty.cmp(&b.counterparty))
        });
        ConflictReport { findings }
    }
}

/// Build the graph and run the pre-matter conflict check in one call.
/// This is the entry point the project-create paths use.
///
/// # Errors
///
/// Returns any `sea_orm::DbErr` from loading the graph.
pub async fn check_new_matter(
    db: &Db,
    client_person_id: Uuid,
    entity_id: Uuid,
) -> Result<ConflictReport, sea_orm::DbErr> {
    let graph = build_graph(db).await?;
    Ok(graph.check(client_person_id, entity_id))
}

#[cfg(test)]
mod tests {
    use super::{check_new_matter, Reason, Severity};
    use crate::entity::{person, person_entity_role, project, relationship_edge};
    use crate::test_support::{dri_person, pg, seed_entity};
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use uuid::Uuid;

    async fn person_named(db: &crate::Db, name: &str) -> Uuid {
        person::ActiveModel {
            name: ActiveValue::Set(name.into()),
            email: ActiveValue::Set(format!("{}@example.com", Uuid::now_v7())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn open_project(db: &crate::Db, entity_id: Uuid, client_id: Uuid) {
        let staff = dri_person(db).await;
        project::ActiveModel {
            name: ActiveValue::Set("Existing matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(entity_id),
            staff_dri_person_id: ActiveValue::Set(Some(staff)),
            client_dri_person_id: ActiveValue::Set(Some(client_id)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn clean_matter_has_no_findings() {
        let db = pg().await;
        let entity_id = seed_entity(&db).await;
        let client = person_named(&db, "Fresh Client").await;
        let report = check_new_matter(&db, client, entity_id).await.unwrap();
        assert!(report.is_clear(), "findings: {:?}", report.summary_lines());
    }

    #[tokio::test]
    async fn repeat_client_on_their_own_entity_is_not_a_conflict() {
        let db = pg().await;
        let entity_id = seed_entity(&db).await;
        let client = person_named(&db, "Returning Client").await;
        // The same client already has an open matter on the same entity —
        // opening another for them is not a conflict with themselves.
        open_project(&db, entity_id, client).await;
        let report = check_new_matter(&db, client, entity_id).await.unwrap();
        assert!(report.is_clear(), "findings: {:?}", report.summary_lines());
    }

    #[tokio::test]
    async fn shared_entity_with_a_different_client_is_a_review() {
        let db = pg().await;
        let entity_id = seed_entity(&db).await;
        let existing = person_named(&db, "Existing Client").await;
        let proposed = person_named(&db, "Proposed Client").await;
        // The firm already runs a matter on this entity for someone else.
        open_project(&db, entity_id, existing).await;
        let report = check_new_matter(&db, proposed, entity_id).await.unwrap();
        assert!(!report.is_clear());
        assert!(!report.has_blocking());
        assert!(report
            .findings
            .iter()
            .any(|f| f.reason == Reason::SharedParty && f.severity == Severity::Review));
    }

    #[tokio::test]
    async fn direct_adverse_edge_to_a_current_client_blocks() {
        let db = pg().await;
        let proposed = person_named(&db, "New Client").await;
        let opponent = person_named(&db, "Opposing Party").await;
        // The opponent is already a client of the firm…
        let opp_entity = seed_entity(&db).await;
        open_project(&db, opp_entity, opponent).await;
        // …and the proposed client is directly adverse to them.
        relationship_edge::ActiveModel {
            from_type: ActiveValue::Set(relationship_edge::NODE_PERSON.into()),
            from_id: ActiveValue::Set(proposed),
            to_type: ActiveValue::Set(relationship_edge::NODE_PERSON.into()),
            to_id: ActiveValue::Set(opponent),
            kind: ActiveValue::Set(relationship_edge::KIND_ADVERSE_TO.into()),
            confidence_pct: ActiveValue::Set(100),
            source_kind: ActiveValue::Set(relationship_edge::SOURCE_MANUAL.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let new_entity = seed_entity(&db).await;
        let report = check_new_matter(&db, proposed, new_entity).await.unwrap();
        assert!(
            report.has_blocking(),
            "findings: {:?}",
            report.summary_lines()
        );
        assert!(report
            .findings
            .iter()
            .any(|f| f.reason == Reason::Adverse && f.severity == Severity::Block));
    }

    #[tokio::test]
    async fn low_confidence_adverse_edge_only_warns() {
        let db = pg().await;
        let proposed = person_named(&db, "Maybe Client").await;
        let opponent = person_named(&db, "Maybe Opponent").await;
        let opp_entity = seed_entity(&db).await;
        open_project(&db, opp_entity, opponent).await;
        // A shaky LLM-parsed adverse edge: below the block floor.
        relationship_edge::ActiveModel {
            from_type: ActiveValue::Set(relationship_edge::NODE_PERSON.into()),
            from_id: ActiveValue::Set(proposed),
            to_type: ActiveValue::Set(relationship_edge::NODE_PERSON.into()),
            to_id: ActiveValue::Set(opponent),
            kind: ActiveValue::Set(relationship_edge::KIND_ADVERSE_TO.into()),
            confidence_pct: ActiveValue::Set(40),
            source_kind: ActiveValue::Set(relationship_edge::SOURCE_LLM.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let new_entity = seed_entity(&db).await;
        let report = check_new_matter(&db, proposed, new_entity).await.unwrap();
        assert!(!report.is_clear());
        assert!(
            !report.has_blocking(),
            "a 40% edge should not hard-block: {:?}",
            report.summary_lines()
        );
    }

    #[tokio::test]
    async fn adversity_through_a_managed_entity_is_caught() {
        let db = pg().await;
        // Proposed client manages an entity that is adverse to an entity
        // the firm already serves for another client — a two-hop chain.
        let proposed = person_named(&db, "Chain Client").await;
        let proposed_entity = seed_entity(&db).await;
        person_entity_role::ActiveModel {
            person_id: ActiveValue::Set(proposed),
            entity_id: ActiveValue::Set(proposed_entity),
            role: ActiveValue::Set("manages".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let opp_entity = seed_entity(&db).await;
        let existing_client = person_named(&db, "Served Client").await;
        open_project(&db, opp_entity, existing_client).await;
        relationship_edge::ActiveModel {
            from_type: ActiveValue::Set(relationship_edge::NODE_ENTITY.into()),
            from_id: ActiveValue::Set(proposed_entity),
            to_type: ActiveValue::Set(relationship_edge::NODE_ENTITY.into()),
            to_id: ActiveValue::Set(opp_entity),
            kind: ActiveValue::Set(relationship_edge::KIND_ADVERSE_TO.into()),
            confidence_pct: ActiveValue::Set(100),
            source_kind: ActiveValue::Set(relationship_edge::SOURCE_MANUAL.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let report = check_new_matter(&db, proposed, proposed_entity)
            .await
            .unwrap();
        assert!(
            report.findings.iter().any(|f| f.reason == Reason::Adverse),
            "expected an adverse finding via the managed entity: {:?}",
            report.summary_lines()
        );
    }
}
