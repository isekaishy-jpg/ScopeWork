//! Leases on a project's named tool resources (phase 2). A lease lives as long as its holder's
//! connection. Requests with the same key and resources that wait when a lease is granted join
//! it: the holder receives their payloads, does the work once and reports a result per payload;
//! requests without a payload are covered by the holder's overall result. Several resources
//! are granted together or not at all, and a waiting request reserves its resources against
//! later ones, so nobody holds a resource while waiting and requests cannot starve.
use super::{Class, MERGE_BYTES, MERGE_OPERATIONS};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// The largest payload one request may carry.
pub(crate) const PAYLOAD_BYTES: usize = 1 << 20;
/// Why a larger payload is refused.
pub(crate) const PAYLOAD_TOO_LARGE: &str = "lease_refused: a payload may not exceed 1 MB";

/// What a waiting request's connection is told.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Event {
    /// The request holds the lease: do the work for these payloads (its own first).
    Granted { lease: u64, payloads: Vec<Value> },
    /// Another holder did the work: its result for this request's payload.
    Done { by: u64, result: Value },
    /// Another holder's work covered this payload-less request.
    Covered { by: u64, result: Value },
}

pub(super) struct Request {
    pub id: u64,
    pub kind: String,
    pub resources: BTreeSet<String>,
    pub key: Option<String>,
    pub class: Class,
    pub submitter: String,
    pub payload: Option<Value>,
    pub bytes: usize,
    pub queued: Instant,
    pub reply: Sender<Event>,
}
impl Request {
    fn rank(&self, now: Instant) -> u64 {
        self.class.aged(self.queued, now)
    }
    fn entry(&self) -> Value {
        json!({"payload_id": self.id, "submitter": self.submitter, "kind": self.kind, "payload": self.payload})
    }
}

struct Held {
    lease: u64,
    holder: Request,
    joined: Vec<Request>,
    since: Instant,
    reported: bool,
}

/// Per kind: leases granted, payloads joined, waits and holds (milliseconds).
#[derive(Default)]
pub(super) struct LeaseStats {
    kinds: BTreeMap<String, (u64, u64, Vec<u64>, Vec<u64>)>,
}
impl LeaseStats {
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
    pub fn view(&self) -> Value {
        Value::Object(
            self.kinds
                .iter()
                .map(|(kind, (leases, joined, waits, holds))| {
                    (
                        kind.clone(),
                        json!({"leases": leases, "joined": joined, "wait_ms": super::spread(waits), "hold_ms": super::spread(holds)}),
                    )
                })
                .collect(),
        )
    }
}

#[derive(Default)]
pub(super) struct Leases {
    waiting: Vec<Request>,
    held: Vec<Held>,
    pub stats: LeaseStats,
    /// When each resource was last granted or released, for drift detection.
    touched: BTreeMap<String, Instant>,
}

fn millis(time: Duration) -> u64 {
    u64::try_from(time.as_millis()).unwrap_or(u64::MAX)
}

impl Leases {
    pub fn enqueue(&mut self, request: Request) {
        self.waiting.push(request);
        self.grant(Instant::now());
    }
    /// Grants every request that can run now, in queue order, joining same-key requests.
    fn grant(&mut self, now: Instant) {
        loop {
            let mut order: Vec<usize> = (0..self.waiting.len()).collect();
            order.sort_by_key(|&i| (self.waiting[i].rank(now), self.waiting[i].id));
            let mut busy: BTreeSet<String> = self
                .held
                .iter()
                .flat_map(|h| h.holder.resources.iter().cloned())
                .collect();
            let mut next = None;
            for i in order {
                let request = &self.waiting[i];
                if request.resources.iter().any(|r| busy.contains(r)) {
                    // Waiting: its resources stay reserved for it against later requests.
                    busy.extend(request.resources.iter().cloned());
                } else {
                    next = Some(i);
                    break;
                }
            }
            let Some(i) = next else { return };
            let holder = self.waiting.remove(i);
            let mut joined = vec![];
            if holder.key.is_some() {
                let mut bytes = holder.bytes;
                self.waiting.sort_by_key(|r| r.id);
                let mut j = 0;
                while j < self.waiting.len() && joined.len() + 1 < MERGE_OPERATIONS {
                    let other = &self.waiting[j];
                    if other.key == holder.key
                        && other.resources == holder.resources
                        && bytes + other.bytes <= MERGE_BYTES
                    {
                        bytes += other.bytes;
                        joined.push(self.waiting.remove(j));
                    } else {
                        j += 1;
                    }
                }
            }
            let lease = holder.id;
            let payloads: Vec<Value> = std::iter::once(&holder)
                .chain(joined.iter())
                .filter(|r| r.payload.is_some() || r.id == holder.id)
                .map(Request::entry)
                .collect();
            let stats = self.stats.kinds.entry(holder.kind.clone()).or_default();
            stats.0 += 1;
            stats.1 += joined.len() as u64;
            for r in std::iter::once(&holder).chain(joined.iter()) {
                stats.2.push(millis(now.duration_since(r.queued)));
            }
            for resource in &holder.resources {
                self.touched.insert(resource.clone(), now);
            }
            let granted = holder
                .reply
                .send(Event::Granted { lease, payloads })
                .is_ok();
            self.held.push(Held {
                lease,
                holder,
                joined,
                since: now,
                reported: false,
            });
            if !granted {
                // The holder's connection is gone already.
                self.release(lease);
                return;
            }
        }
    }
    /// The holder's results: one per payload (`{"payload_id", "ok"}` or `{"payload_id",
    /// "error"}`), plus an overall result for covered requests.
    pub fn results(
        &mut self,
        lease: u64,
        results: &[Value],
        overall: &Value,
    ) -> Result<(), String> {
        let held = self
            .held
            .iter_mut()
            .find(|h| h.lease == lease)
            .ok_or_else(|| format!("No lease {lease} is held"))?;
        if held.reported {
            return Err(format!("Lease {lease} already reported its results"));
        }
        held.reported = true;
        for request in held.joined.drain(..) {
            let event = if request.payload.is_some() {
                let result = results
                    .iter()
                    .find(|r| r["payload_id"].as_u64() == Some(request.id))
                    .map(|r| {
                        let mut r = r.clone();
                        if let Some(object) = r.as_object_mut() {
                            object.remove("payload_id");
                        }
                        r
                    })
                    .filter(|r| r.get("ok").is_some() || r.get("error").is_some())
                    .unwrap_or_else(|| json!({"error": "no result"}));
                Event::Done { by: lease, result }
            } else {
                Event::Covered {
                    by: lease,
                    result: overall.clone(),
                }
            };
            let _ = request.reply.send(event);
        }
        Ok(())
    }
    /// Ends a lease. Joined requests whose results were never reported wait again at their
    /// original place; their clients may be granted the next lease themselves.
    pub fn release(&mut self, lease: u64) {
        let Some(position) = self.held.iter().position(|h| h.lease == lease) else {
            return;
        };
        let held = self.held.remove(position);
        let now = Instant::now();
        for resource in &held.holder.resources {
            self.touched.insert(resource.clone(), now);
        }
        if let Some(stats) = self.stats.kinds.get_mut(&held.holder.kind) {
            stats.3.push(millis(held.since.elapsed()));
        }
        if !held.reported {
            self.waiting.extend(held.joined);
        }
        self.grant(Instant::now());
    }
    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty() && self.held.is_empty()
    }
    /// Whether a lease on `resource` is held now, or was granted or released since `since`:
    /// then a change to its files was its holder's.
    pub fn excused(&self, resource: &str, since: Instant) -> bool {
        self.held
            .iter()
            .any(|h| h.holder.resources.contains(resource))
            || self.touched.get(resource).is_some_and(|t| *t >= since)
    }
    /// Held and waiting leases, for `queue` and `job status`.
    pub fn view(&self) -> Value {
        let now = Instant::now();
        let held: Vec<Value> = self
            .held
            .iter()
            .map(|h| {
                json!({"lease": h.lease, "kind": h.holder.kind, "resources": h.holder.resources,
                    "key": h.holder.key, "class": h.holder.class, "submitter": h.holder.submitter,
                    "joined": h.joined.len(), "held_ms": millis(now.duration_since(h.since))})
            })
            .collect();
        let mut order: Vec<&Request> = self.waiting.iter().collect();
        order.sort_by_key(|r| (r.rank(now), r.id));
        let waiting: Vec<Value> = order
            .iter()
            .enumerate()
            .map(|(i, r)| {
                json!({"request": r.id, "kind": r.kind, "resources": r.resources, "key": r.key,
                    "class": r.class, "submitter": r.submitter, "position": i + 1,
                    "waited_ms": millis(now.duration_since(r.queued))})
            })
            .collect();
        json!({"held": held, "waiting": waiting})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};

    fn request(
        id: u64,
        resources: &[&str],
        key: Option<&str>,
        payload: Option<Value>,
    ) -> (Request, Receiver<Event>) {
        let (reply, events) = channel();
        let bytes = payload.as_ref().map_or(0, |p| p.to_string().len());
        (
            Request {
                id,
                kind: "test".into(),
                resources: resources.iter().map(|r| r.to_string()).collect(),
                key: key.map(str::to_string),
                class: Class::Normal,
                submitter: format!("agent-{id}"),
                payload,
                bytes,
                queued: Instant::now(),
                reply,
            },
            events,
        )
    }

    #[test]
    fn same_key_requests_join_and_each_gets_its_own_result() {
        let mut leases = Leases::default();
        let (a, a_events) = request(1, &["ledger"], Some("put"), Some(json!({"row": 1})));
        leases.enqueue(a);
        assert!(matches!(
            a_events.try_recv(),
            Ok(Event::Granted { lease: 1, .. })
        ));
        // While 1 holds the ledger, three more arrive: two puts and a payload-less apply.
        let (b, b_events) = request(2, &["ledger"], Some("put"), Some(json!({"row": 2})));
        let (c, c_events) = request(3, &["ledger"], Some("put"), Some(json!({"row": 3})));
        let (d, d_events) = request(4, &["ledger"], Some("put"), None);
        leases.enqueue(b);
        leases.enqueue(c);
        leases.enqueue(d);
        leases.results(1, &[], &json!({})).unwrap();
        leases.release(1);
        // 2 holds the next lease with 3's and 4's requests joined.
        let Ok(Event::Granted { lease: 2, payloads }) = b_events.try_recv() else {
            panic!("2 was not granted");
        };
        let ids: Vec<u64> = payloads
            .iter()
            .map(|p| p["payload_id"].as_u64().unwrap())
            .collect();
        assert_eq!(
            ids,
            [2, 3],
            "the holder's own payload first, payload-less requests omitted"
        );
        assert!(c_events.try_recv().is_err() && d_events.try_recv().is_err());
        leases
            .results(
                2,
                &[json!({"payload_id": 2, "ok": "two"})],
                &json!({"applied": true}),
            )
            .unwrap();
        assert_eq!(
            c_events.try_recv(),
            Ok(Event::Done {
                by: 2,
                result: json!({"error": "no result"})
            })
        );
        assert_eq!(
            d_events.try_recv(),
            Ok(Event::Covered {
                by: 2,
                result: json!({"applied": true})
            })
        );
        leases.release(2);
        assert!(leases.is_empty());
    }

    #[test]
    fn only_requests_waiting_at_the_grant_join_it() {
        let mut leases = Leases::default();
        let (a, _a_events) = request(1, &["ghidra"], Some("apply"), None);
        leases.enqueue(a);
        // Arrives while 1 runs: it must not be covered by work that may predate its request.
        let (b, b_events) = request(2, &["ghidra"], Some("apply"), None);
        leases.enqueue(b);
        leases.results(1, &[], &json!({"applied": 1})).unwrap();
        assert!(b_events.try_recv().is_err());
        leases.release(1);
        assert!(matches!(
            b_events.try_recv(),
            Ok(Event::Granted { lease: 2, .. })
        ));
    }

    #[test]
    fn several_resources_are_granted_together_and_reserved_while_waiting() {
        let mut leases = Leases::default();
        let (ghidra, _g) = request(1, &["ghidra"], None, None);
        leases.enqueue(ghidra);
        // Needs both: waits for ghidra without taking the free ledger.
        let (apply, apply_events) = request(2, &["ghidra", "ledger"], None, None);
        leases.enqueue(apply);
        // A later ledger-only request may not overtake the waiting apply on the ledger.
        let (put, put_events) = request(3, &["ledger"], None, None);
        leases.enqueue(put);
        assert!(apply_events.try_recv().is_err() && put_events.try_recv().is_err());
        leases.release(1);
        assert!(matches!(
            apply_events.try_recv(),
            Ok(Event::Granted { lease: 2, .. })
        ));
        assert!(put_events.try_recv().is_err());
        leases.release(2);
        assert!(matches!(
            put_events.try_recv(),
            Ok(Event::Granted { lease: 3, .. })
        ));
    }

    #[test]
    fn a_lost_holder_returns_its_joined_requests_to_their_place() {
        let mut leases = Leases::default();
        let (a, _a) = request(1, &["ledger"], Some("put"), Some(json!(1)));
        leases.enqueue(a);
        let (b, b_events) = request(2, &["ledger"], Some("put"), Some(json!(2)));
        let (c, c_events) = request(3, &["ledger"], Some("put"), Some(json!(3)));
        leases.enqueue(b);
        leases.enqueue(c);
        leases.release(1);
        assert!(matches!(
            b_events.try_recv(),
            Ok(Event::Granted { lease: 2, .. })
        ));
        // Holder 2 dies before reporting: 3 is not answered but waits again, first in line.
        let (d, d_events) = request(4, &["ledger"], Some("other"), None);
        leases.enqueue(d);
        leases.release(2);
        assert!(matches!(
            c_events.try_recv(),
            Ok(Event::Granted { lease: 3, .. })
        ));
        assert!(d_events.try_recv().is_err());
        // A holder whose connection is already gone is released at once.
        drop(d_events);
        leases.release(3);
        assert!(leases.is_empty());
    }
}
