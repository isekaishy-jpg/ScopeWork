//! Which records a change reopens.
//!
//! A complete record's acceptance depends on its required children and its prerequisites, so a
//! change must reopen every complete record it reaches through those relations: ancestors of a
//! changed record (under its old and new parent) and prerequisite consumers of a changed
//! function, transitively through records it reopens. A record that is not complete is left
//! untouched: it has no acceptance to lose, and its completion token will see the change
//! through the changed records' own revisions. Bumping such records only produced conflicts
//! and journal noise. A context-only record is inventory, not part of its parent's required
//! research, so a change to it does not reach its parent either. Pooled records are reopened
//! like any other: a pool decides who may change a record, not how changes reach it.
use super::*;
use std::collections::{HashMap, HashSet};

/// Reopens the complete records that `changed` reaches in `after` and returns them. The
/// changed records themselves are the caller's to update. `old_parents` gives the previous
/// parent of moved or removed records; `consumers` maps a function to the functions that
/// tag it as a prerequisite. A context-only record that changed reaches only its consumers
/// (they stamp its text) and its old parent (which lost it), not its current parent, whose
/// completion cannot depend on it.
pub(super) fn reopen_affected(
    after: &mut Project,
    changed: &HashSet<u64>,
    old_parents: &HashMap<u64, u64>,
    consumers: &HashMap<u64, Vec<u64>>,
) -> Vec<u64> {
    let positions: HashMap<u64, usize> = after
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect();
    let reached = |id: u64, after: &Project| -> Vec<u64> {
        let mut next: Vec<u64> = consumers.get(&id).cloned().unwrap_or_default();
        // A parent's completion depends on its required children only, not on a context-only
        // child (inventory).
        if let Some(node) = positions.get(&id).map(|i| &after.nodes[*i])
            && !node.context_only
            && let Some(parent) = node.parent
        {
            next.push(parent);
        }
        if let Some(parent) = old_parents.get(&id) {
            next.push(*parent);
        }
        next
    };
    let mut queue: Vec<u64> = changed.iter().flat_map(|id| reached(*id, after)).collect();
    let mut seen: HashSet<u64> = changed.clone();
    let mut reopened = vec![];
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(&i) = positions.get(&id) else {
            continue;
        };
        let node = &mut after.nodes[i];
        if node.status != Status::Complete {
            continue;
        }
        node.status = Status::InProgress;
        node.revision += 1;
        node.accepted_hash = None;
        node.accepted_prerequisites.clear();
        reopened.push(id);
        queue.extend(reached(id, after));
    }
    reopened
}

/// Prerequisite consumers by function, from a list of edges.
pub(super) fn consumers_of(edges: &[Dependency]) -> HashMap<u64, Vec<u64>> {
    let mut consumers: HashMap<u64, Vec<u64>> = HashMap::new();
    for edge in edges {
        consumers
            .entry(edge.prerequisite)
            .or_default()
            .push(edge.dependent);
    }
    consumers
}

/// Marks a changed record as needing review: a new revision, no acceptance, and status
/// `status` (or In Progress when it was complete and no status is given).
pub(super) fn mark_changed(node: &mut Node, status: Option<Status>) {
    node.revision += 1;
    node.accepted_hash = None;
    node.accepted_prerequisites.clear();
    node.status = status.unwrap_or(if node.status == Status::Complete {
        Status::InProgress
    } else {
        node.status
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, parent: Option<u64>, kind: Kind, status: Status) -> Node {
        Node {
            context_only: false,
            id,
            parent,
            kind,
            name: format!("N{id}"),
            status,
            rva: (kind == Kind::Function).then_some(0x1000 + id as u32),
            revision: 1,
            accepted_hash: (status == Status::Complete).then(|| "accepted".into()),
            accepted_prerequisites: Default::default(),
        }
    }
    /// App 0 → architecture 1 → chunks 2 (complete) and 3 (in progress); functions 4 and 5
    /// in chunk 2, 6 in chunk 3; function 5 is a prerequisite of 6 and 4 of 5.
    fn project() -> Project {
        Project {
            layout_version: 5,
            id: "fixture".into(),
            name: "Fixture".into(),
            binary: "synthetic".into(),
            research_scope: String::new(),
            next_id: 7,
            revision: 1,
            structural_epoch: 0,
            active_import: None,
            last_import: None,
            nodes: vec![
                node(0, None, Kind::App, Status::InProgress),
                node(1, Some(0), Kind::Architecture, Status::InProgress),
                node(2, Some(1), Kind::Chunk, Status::Complete),
                node(3, Some(1), Kind::Chunk, Status::InProgress),
                node(4, Some(2), Kind::Function, Status::Complete),
                node(5, Some(2), Kind::Function, Status::Complete),
                node(6, Some(3), Kind::Function, Status::Complete),
            ],
            dependencies: vec![
                Dependency {
                    dependent: 6,
                    prerequisite: 5,
                },
                Dependency {
                    dependent: 5,
                    prerequisite: 4,
                },
            ],
        }
    }
    fn revision(p: &Project, id: u64) -> u64 {
        p.nodes.iter().find(|n| n.id == id).unwrap().revision
    }

    #[test]
    fn reopens_complete_ancestors_and_consumers_only() {
        let mut p = project();
        let consumers = consumers_of(&p.dependencies);
        let mut reopened = reopen_affected(&mut p, &[4].into(), &HashMap::new(), &consumers);
        reopened.sort_unstable();
        // Chunk 2 (complete ancestor), 5 (consumer of 4) and 6 (consumer of 5) reopen.
        assert_eq!(reopened, vec![2, 5, 6]);
        // Incomplete ancestors keep their revision: architecture 1, App 0 and chunk 3.
        for id in [0, 1, 3] {
            assert_eq!(revision(&p, id), 1, "node {id} bumped");
        }
        assert!(
            p.nodes
                .iter()
                .all(|n| n.status != Status::Complete || n.id == 4)
        );
    }

    #[test]
    fn a_walk_stops_at_records_that_are_not_complete() {
        let mut p = project();
        p.nodes[5].status = Status::InProgress; // function 5: its consumer 6 cannot depend on it
        let consumers = consumers_of(&p.dependencies);
        let reopened = reopen_affected(&mut p, &[4].into(), &HashMap::new(), &consumers);
        assert_eq!(reopened, vec![2]);
        assert_eq!(revision(&p, 6), 1);
    }

    #[test]
    fn moved_records_reach_their_old_parent() {
        let mut p = project();
        // Function 4 now sits in chunk 3; its old chunk 2 is complete and must reopen.
        p.nodes[4].parent = Some(3);
        let reopened = reopen_affected(&mut p, &[4].into(), &[(4, 2)].into(), &HashMap::new());
        assert_eq!(reopened, vec![2]);
    }
}
