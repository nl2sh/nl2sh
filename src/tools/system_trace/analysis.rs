//! Supported wire fields follow Perfetto v48.1 protos/perfetto/trace/{ftrace,ps,android}.
//! https://github.com/google/perfetto/tree/v48.1/protos/perfetto/trace
//! No SQL engine, external trace processor, or arbitrary protobuf recursion is used.
use super::wire::{bytes, fields, num, string};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_EVENTS: usize = 500_000;
const MAX_THREADS: usize = 32_768;
const MAX_PACKET: usize = 4 * 1024 * 1024;
const MAX_FINDINGS: usize = 50;

#[derive(Clone)]
enum Kind {
    Switch {
        prev: u32,
        state: u64,
        next: u32,
    },
    Wake(u32),
    Print(String),
    Send {
        id: u64,
        target: u32,
        reply: bool,
        oneway: bool,
    },
    Receive(u64),
    Frame {
        pid: u32,
        token: u64,
        present: u64,
        jank: u64,
    },
    Loss,
}
#[derive(Clone)]
struct Event {
    ts: u64,
    tid: u32,
    cpu: u32,
    kind: Kind,
}
#[derive(Default)]
struct Thread {
    tgid: u32,
    name: String,
}
#[derive(Default, Serialize)]
struct Coverage {
    packets: usize,
    sched_switch: usize,
    sched_wakeup: usize,
    atrace: usize,
    binder_transaction: usize,
    binder_received: usize,
    frame_timeline: usize,
    lost_markers: usize,
    unlocated_loss_markers: usize,
    unsupported_compact_bundles: usize,
    unsupported_clock_bundles: usize,
    unsupported_clock_packets: usize,
    compressed_packets: usize,
    unsupported_packets: usize,
}
#[derive(Serialize)]
struct Finding {
    timestamp_ns: u64,
    tid: u32,
    process_id: u32,
    thread_name: String,
    duration_ms: Option<f64>,
    evidence: String,
}
#[derive(Default, Serialize)]
struct Group {
    count: usize,
    examples: Vec<Finding>,
}
impl Group {
    fn add(&mut self, finding: Finding) {
        self.count += 1;
        if self.examples.len() < MAX_FINDINGS {
            self.examples.push(finding);
        }
    }
}
#[derive(Default, Serialize)]
struct CpuThread {
    tid: u32,
    process_id: u32,
    name: String,
    running_ms: f64,
    wakeups: usize,
    wakeups_per_second: f64,
    runnable_wait_ms: f64,
}

#[derive(Serialize)]
struct Report {
    status: &'static str,
    coverage: Coverage,
    observed_duration_ms: f64,
    target_pid: Option<u32>,
    threshold_ms: u64,
    frame_budget_ms: f64,
    main_thread_unscheduled: Group,
    render_thread_anomalies: Group,
    frame_anomalies: Group,
    binder_delivery_delays: Group,
    cpu_contention: Group,
    excessive_wakeups: Group,
    cpu_threads: Vec<CpuThread>,
    limitations: Vec<String>,
}

struct Trace {
    events: Vec<Event>,
    threads: HashMap<u32, Thread>,
    processes: HashMap<u32, String>,
    coverage: Coverage,
}
impl Trace {
    fn thread(&mut self, tid: u32) -> Result<&mut Thread> {
        if self.threads.len() >= MAX_THREADS && !self.threads.contains_key(&tid) {
            bail!("trace thread budget exceeded")
        }
        Ok(self.threads.entry(tid).or_default())
    }
    fn push(&mut self, event: Event) -> Result<()> {
        if self.events.len() >= MAX_EVENTS {
            bail!("trace event budget exceeded (500000); use a shorter capture")
        }
        self.events.push(event);
        Ok(())
    }
    fn loss(&mut self, ts: u64) -> Result<()> {
        self.coverage.lost_markers += 1;
        self.push(Event {
            ts,
            tid: 0,
            cpu: 0,
            kind: Kind::Loss,
        })
    }
    fn process_tree(&mut self, input: &[u8]) -> Result<()> {
        for field in fields(input)? {
            match field.id {
                1 => {
                    let p = fields(field.bytes)?;
                    if let Some(pid) = num(&p, 1).and_then(|p| u32::try_from(p).ok()) {
                        if self.processes.len() >= MAX_THREADS {
                            bail!("trace process budget exceeded")
                        }
                        self.processes.insert(pid, string(&p, 3));
                        self.thread(pid)?.tgid = pid;
                    }
                }
                2 => {
                    let t = fields(field.bytes)?;
                    if let (Some(tid), Some(tgid)) = (num(&t, 1), num(&t, 3)) {
                        let thread = self.thread(tid as u32)?;
                        thread.tgid = tgid as u32;
                        let name = string(&t, 2);
                        if !name.is_empty() {
                            thread.name = name;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn ftrace(&mut self, input: &[u8]) -> Result<()> {
        let bundle = fields(input)?;
        let cpu = num(&bundle, 1).unwrap_or(0) as u32;
        if cpu > 4095 {
            bail!("trace CPU budget exceeded")
        }
        // Non-boot clocks require conversion; do not compare across CPU clock domains.
        if num(&bundle, 5).unwrap_or(0) != 0 {
            self.coverage.unsupported_clock_bundles += 1;
            return Ok(());
        }
        if bytes(&bundle, 4).is_some() {
            self.coverage.unsupported_compact_bundles += 1;
        }
        let lost = num(&bundle, 3).unwrap_or(0) != 0 || bytes(&bundle, 4).is_some();
        let mut inserted_loss = false;
        for field in bundle.iter().filter(|f| f.id == 2) {
            let e = fields(field.bytes)?;
            let Some(ts) = num(&e, 1) else { continue };
            let tid = num(&e, 2).unwrap_or(0) as u32;
            if lost && !inserted_loss {
                self.loss(ts)?;
                inserted_loss = true;
            }
            for payload in &e {
                let kind = match payload.id {
                    4 => {
                        let p = fields(payload.bytes)?;
                        let (Some(prev), Some(state), Some(next)) =
                            (num(&p, 2), num(&p, 4), num(&p, 6))
                        else {
                            continue;
                        };
                        self.thread(prev as u32)?.name = string(&p, 1);
                        self.thread(next as u32)?.name = string(&p, 5);
                        self.coverage.sched_switch += 1;
                        Kind::Switch {
                            prev: prev as u32,
                            state,
                            next: next as u32,
                        }
                    }
                    17 => {
                        let p = fields(payload.bytes)?;
                        if num(&p, 4) == Some(0) {
                            continue;
                        }
                        let Some(pid) = num(&p, 2) else { continue };
                        self.thread(pid as u32)?.name = string(&p, 1);
                        self.coverage.sched_wakeup += 1;
                        Kind::Wake(pid as u32)
                    }
                    3 => {
                        let p = fields(payload.bytes)?;
                        // Read markers before removing controls: newline is a valid print terminator.
                        let marker = bytes(&p, 2)
                            .and_then(|b| std::str::from_utf8(b).ok())
                            .unwrap_or("");
                        if marker.len() > 1024 {
                            continue;
                        }
                        if let Some(rest) = marker.strip_prefix("B|") {
                            if let Some((pid, _)) = rest.split_once('|') {
                                if let Ok(pid) = pid.parse::<u32>() {
                                    self.thread(tid)?.tgid = pid;
                                }
                            }
                        }
                        self.coverage.atrace += 1;
                        Kind::Print(marker.trim_end().to_owned())
                    }
                    50 => {
                        let p = fields(payload.bytes)?;
                        let Some(id) = num(&p, 1) else { continue };
                        self.coverage.binder_transaction += 1;
                        Kind::Send {
                            id,
                            target: num(&p, 3).unwrap_or(0) as u32,
                            reply: num(&p, 5).unwrap_or(0) != 0,
                            oneway: num(&p, 7).unwrap_or(0) & 1 != 0,
                        }
                    }
                    51 => {
                        let p = fields(payload.bytes)?;
                        let Some(id) = num(&p, 1) else { continue };
                        self.coverage.binder_received += 1;
                        Kind::Receive(id)
                    }
                    _ => continue,
                };
                self.push(Event { ts, tid, cpu, kind })?;
            }
        }
        if lost && !inserted_loss {
            self.coverage.lost_markers += 1;
        }
        Ok(())
    }
}

fn decode(input: &[u8]) -> Result<Trace> {
    let mut trace = Trace {
        events: Vec::new(),
        threads: HashMap::new(),
        processes: HashMap::new(),
        coverage: Coverage::default(),
    };
    // Trace is repeated TracePacket field 1. Packet and event budgets also bound allocation.
    let mut rest = input;
    while !rest.is_empty() {
        let key = super::wire::varint(&mut rest)?;
        if key != 10 {
            bail!("expected Perfetto Trace.packet field (not a supported raw trace)")
        }
        let size =
            usize::try_from(super::wire::varint(&mut rest)?).context("packet length overflow")?;
        if size > MAX_PACKET {
            bail!("trace packet exceeds 4 MiB")
        }
        let packet = rest.get(..size).context("truncated TracePacket")?;
        rest = &rest[size..];
        trace.coverage.packets += 1;
        if trace.coverage.packets > 100_000 {
            bail!("trace packet budget exceeded")
        }
        let p = fields(packet)?;
        if num(&p, 42).unwrap_or(0) != 0 {
            if let Some(ts) = num(&p, 8) {
                trace.loss(ts)?;
            } else if let Some(bundle) = bytes(&p, 1) {
                let bundle = fields(bundle)?;
                let ts = bundle
                    .iter()
                    .filter(|f| f.id == 2)
                    .find_map(|f| fields(f.bytes).ok().and_then(|e| num(&e, 1)));
                if let Some(ts) = ts {
                    trace.loss(ts)?;
                } else {
                    trace.coverage.unlocated_loss_markers += 1;
                }
            } else {
                trace.coverage.unlocated_loss_markers += 1;
            }
        }
        let mut supported = false;
        for f in &p {
            match f.id {
                1 => {
                    trace.ftrace(f.bytes)?;
                    supported = true;
                }
                2 => {
                    trace.process_tree(f.bytes)?;
                    supported = true;
                }
                50 | 133 => {
                    trace.coverage.compressed_packets += 1;
                }
                76 => {
                    supported = true;
                    if !matches!(num(&p, 58), None | Some(6)) {
                        trace.coverage.unsupported_clock_packets += 1;
                        continue;
                    }
                    let Some(ts) = num(&p, 8) else { continue };
                    for frame in fields(f.bytes)?.iter().filter(|f| f.id == 2 || f.id == 4) {
                        let values = fields(frame.bytes)?;
                        let surface = frame.id == 4;
                        let pid = num(&values, if surface { 4 } else { 3 }).unwrap_or(0) as u32;
                        trace.coverage.frame_timeline += 1;
                        trace.push(Event {
                            ts,
                            tid: pid,
                            cpu: 0,
                            kind: Kind::Frame {
                                pid,
                                token: num(&values, 2).unwrap_or(0),
                                present: num(&values, if surface { 6 } else { 4 }).unwrap_or(0),
                                jank: num(&values, if surface { 9 } else { 7 }).unwrap_or(0),
                            },
                        })?;
                    }
                }
                _ => {}
            }
        }
        if !supported {
            trace.coverage.unsupported_packets += 1;
        }
    }
    trace.events.sort_by_key(|e| e.ts);
    Ok(trace)
}

fn finding(trace: &Trace, tid: u32, ts: u64, duration: Option<u64>, evidence: String) -> Finding {
    let thread = trace.threads.get(&tid);
    Finding {
        timestamp_ns: ts,
        tid,
        process_id: thread.map_or(0, |t| t.tgid),
        thread_name: thread.map_or_else(String::new, |t| t.name.clone()),
        duration_ms: duration.map(|d| d as f64 / 1e6),
        evidence,
    }
}
fn selected(trace: &Trace, tid: u32, target: Option<u32>) -> bool {
    target.is_none_or(|pid| tid == pid || trace.threads.get(&tid).is_some_and(|t| t.tgid == pid))
}

pub(super) fn analyze(
    input: &[u8],
    target: Option<u32>,
    package: Option<&str>,
    threshold_ms: u64,
    frame_budget_ms: f64,
) -> Result<String> {
    let trace = decode(input)?;
    let target = match (target, package) {
        (Some(pid), _) => Some(pid),
        (None, Some(name)) => {
            let pids = trace
                .processes
                .iter()
                .filter(|(_, command)| command.as_str() == name)
                .map(|(pid, _)| *pid)
                .collect::<Vec<_>>();
            if pids.len() != 1 {
                bail!("package must identify exactly one recorded process; pass pid for multi-process apps")
            }
            pids.first().copied()
        }
        _ => None,
    };
    let first = trace
        .events
        .iter()
        .find(|e| !matches!(e.kind, Kind::Loss))
        .map_or(0, |e| e.ts);
    let last = trace
        .events
        .iter()
        .rev()
        .find(|e| !matches!(e.kind, Kind::Loss))
        .map_or(first, |e| e.ts);
    let duration = last.saturating_sub(first);
    let threshold = threshold_ms * 1_000_000;
    let mut main = Group::default();
    let mut render = Group::default();
    let mut frames = Group::default();
    let mut binder = Group::default();
    let mut contention = Group::default();
    let mut wakeups = Group::default();
    let mut ready: HashMap<u32, u64> = HashMap::new();
    let mut running: HashMap<u32, (u32, u64)> = HashMap::new();
    let mut running_tids = HashSet::new();
    let mut slices: HashMap<u32, Vec<(u64, String)>> = HashMap::new();
    let mut sends: HashMap<u64, (u64, u32, u32, bool, bool)> = HashMap::new();
    let mut stats: BTreeMap<u32, CpuThread> = BTreeMap::new();
    let mut incomplete = 0usize;
    for e in &trace.events {
        match &e.kind {
            Kind::Loss => {
                ready.clear();
                running.clear();
                running_tids.clear();
                slices.clear();
                sends.clear();
            }
            Kind::Wake(tid) => {
                if *tid == 0 {
                    continue;
                }
                let currently_running = running_tids.contains(tid);
                if !currently_running {
                    ready.entry(*tid).or_insert(e.ts);
                }
                stats.entry(*tid).or_default().wakeups += 1;
            }
            Kind::Switch { prev, state, next } => {
                // Only pair on the same CPU when its previous task identity matches.
                if let Some((tid, start)) = running.remove(&e.cpu) {
                    running_tids.remove(&tid);
                    if tid == *prev && tid != 0 {
                        stats.entry(tid).or_default().running_ms +=
                            e.ts.saturating_sub(start) as f64 / 1e6;
                    }
                }
                if *prev != 0 && prev != next {
                    if *state == 0 {
                        ready.entry(*prev).or_insert(e.ts);
                    } else {
                        ready.remove(prev);
                    }
                }
                running.insert(e.cpu, (*next, e.ts));
                running_tids.insert(*next);
                if let Some(start) = ready.remove(next) {
                    let wait = e.ts.saturating_sub(start);
                    stats.entry(*next).or_default().runnable_wait_ms += wait as f64 / 1e6;
                    if wait >= threshold && selected(&trace, *next, target) {
                        let evidence = format!(
                            "runnable/wakeup → scheduled on CPU {}; sleeping intervals excluded",
                            e.cpu
                        );
                        contention.add(finding(&trace, *next, start, Some(wait), evidence.clone()));
                        if trace.threads.get(next).is_some_and(|t| t.tgid == *next)
                            || target == Some(*next)
                        {
                            main.add(finding(&trace, *next, start, Some(wait), evidence.clone()));
                        }
                        if trace
                            .threads
                            .get(next)
                            .is_some_and(|t| t.name == "RenderThread")
                        {
                            render.add(finding(&trace, *next, start, Some(wait), evidence));
                        }
                    }
                }
            }
            Kind::Print(marker) => {
                if let Some(rest) = marker.strip_prefix("B|") {
                    if let Some((pid, name)) = rest.split_once('|') {
                        if pid.parse::<u32>().is_ok() {
                            let thread_stack = slices.entry(e.tid).or_default();
                            if thread_stack.len() < 64 {
                                thread_stack.push((e.ts, name.chars().take(256).collect()));
                            } else {
                                incomplete += thread_stack.len();
                                thread_stack.clear();
                            }
                        }
                    }
                } else if marker == "E" || marker.starts_with("E|") {
                    if let Some((start, name)) = slices.get_mut(&e.tid).and_then(Vec::pop) {
                        let elapsed = e.ts.saturating_sub(start);
                        if selected(&trace, e.tid, target) {
                            if name.starts_with("Choreographer#doFrame")
                                && elapsed as f64 > frame_budget_ms * 1e6
                            {
                                frames.add(finding(&trace, e.tid, start, Some(elapsed), format!("{name}: exceeds configured frame budget; long callback, not proof of dropped presentation")));
                            }
                            if trace
                                .threads
                                .get(&e.tid)
                                .is_some_and(|t| t.name == "RenderThread")
                                && elapsed >= threshold
                            {
                                render.add(finding(
                                    &trace,
                                    e.tid,
                                    start,
                                    Some(elapsed),
                                    format!("long RenderThread atrace slice: {name}"),
                                ));
                            }
                        }
                    } else {
                        incomplete += 1;
                    }
                }
            }
            Kind::Send {
                id,
                target: receiver,
                reply,
                oneway,
            } => {
                sends.insert(*id, (e.ts, e.tid, *receiver, *reply, *oneway));
            }
            Kind::Receive(id) => {
                if let Some((start, sender, receiver, reply, oneway)) = sends.remove(id) {
                    let elapsed = e.ts.saturating_sub(start);
                    if elapsed >= threshold
                        && (selected(&trace, sender, target)
                            || selected(&trace, e.tid, target)
                            || target == Some(receiver))
                    {
                        binder.add(finding(&trace, sender, start, Some(elapsed), format!("transaction {id} sent → received by tid {}; reply={reply} oneway={oneway}; delivery latency, excludes handler/reply round-trip", e.tid)));
                    }
                } else {
                    incomplete += 1;
                }
            }
            Kind::Frame {
                pid,
                token,
                present,
                jank,
            } => {
                if target.is_none_or(|t| t == *pid)
                    && (*present == 2 || *present == 4 || *jank & !1 != 0)
                {
                    frames.add(finding(&trace, *pid, e.ts, None, format!("FrameTimeline token={token} present_type={present} jank_mask={jank}; dropped={} late={} app_deadline_missed={}", *present == 4 || jank & 1024 != 0, *present == 2, jank & 64 != 0)));
                }
            }
        }
        if stats.len() > MAX_THREADS || sends.len() > MAX_EVENTS || slices.len() > MAX_THREADS {
            bail!("analysis state budget exceeded")
        }
    }
    incomplete += slices.values().map(Vec::len).sum::<usize>() + sends.len() + ready.len();
    let seconds = duration as f64 / 1e9;
    let mut cpu_threads = Vec::new();
    for (tid, mut stat) in stats {
        if !selected(&trace, tid, target) {
            continue;
        }
        stat.tid = tid;
        if let Some(t) = trace.threads.get(&tid) {
            stat.process_id = t.tgid;
            stat.name = t.name.clone();
        }
        stat.wakeups_per_second = if seconds > 0.0 {
            stat.wakeups as f64 / seconds
        } else {
            0.0
        };
        if seconds >= 1.0 && stat.wakeups_per_second >= 100.0 {
            wakeups.add(finding(&trace, tid, first, None, format!("{} successful sched_wakeup events over {:.3}s ({:.1}/s); heuristic threshold 100/s, not an energy measurement", stat.wakeups, seconds, stat.wakeups_per_second)));
        }
        cpu_threads.push(stat);
    }
    cpu_threads.sort_by(|a, b| b.running_ms.total_cmp(&a.running_ms));
    cpu_threads.truncate(30);
    let mut limitations = vec![
        "Only standard uncompressed ftrace sched_switch/sched_wakeup/print/binder, ProcessTree and legacy FrameTimeline (field 76) are decoded. Unknown fields are skipped.".into(),
        "Runnable waits show scheduling delay, not all periods without execution. Sleep, I/O, locks and ANR cause are not inferred. CPU contention and wakeup thresholds are heuristics.".into(),
        "Binder metric is send-to-receive delivery latency, not complete synchronous transaction duration. Unmatched and boundary intervals are excluded.".into(),
        "Choreographer duration uses a caller-supplied frame budget; FrameTimeline present/jank flags are separate presentation evidence. Newer TrackEvent-based FrameTimeline is unsupported.".into(),
        "Process/thread IDs may be reused; keep captures short. Kernel comm names can be truncated. Counts are per observed event/slice, not unique display frames.".into(),
        format!("{incomplete} unmatched/open intervals excluded; examples limited to {MAX_FINDINGS} per category; CPU threads limited to 30."),
    ];
    if trace.coverage.sched_switch == 0 {
        limitations.push("Scheduling unavailable: no supported sched_switch events.".into());
    }
    if trace.coverage.sched_wakeup == 0 {
        limitations.push(
            "Wakeup evidence unavailable; only runnable preemption waits can be measured.".into(),
        );
    }
    if trace.coverage.atrace == 0 {
        limitations.push("Atrace unavailable: no supported print markers; Choreographer/RenderThread slice analysis unavailable.".into());
    }
    if trace.coverage.frame_timeline == 0 {
        limitations.push("FrameTimeline unavailable: no supported actual frame events; absence of findings does not prove smooth rendering.".into());
    }
    if trace.coverage.binder_transaction == 0 || trace.coverage.binder_received == 0 {
        limitations.push("Binder latency unavailable: send/receive evidence missing.".into());
    }
    let partial = trace.coverage.lost_markers > 0
        || trace.coverage.unlocated_loss_markers > 0
        || trace.coverage.compressed_packets > 0
        || trace.coverage.unsupported_compact_bundles > 0
        || trace.coverage.unsupported_clock_bundles > 0
        || trace.coverage.unsupported_clock_packets > 0;
    if partial {
        limitations.push("Loss/sequence-start flags or unsupported encoding/clock detected. Results are partial; pairing is reset at located loss timestamps. Unlocated flags cannot be placed on the timeline; duration findings are candidate evidence. Missing data cannot establish absence of an anomaly.".into());
    }
    let report = Report {
        status: if trace.events.is_empty() {
            "unsupported"
        } else if partial {
            "partial"
        } else {
            "observed"
        },
        coverage: trace.coverage,
        observed_duration_ms: duration as f64 / 1e6,
        target_pid: target,
        threshold_ms,
        frame_budget_ms,
        main_thread_unscheduled: main,
        render_thread_anomalies: render,
        frame_anomalies: frames,
        binder_delivery_delays: binder,
        cpu_contention: contention,
        excessive_wakeups: wakeups,
        cpu_threads,
        limitations,
    };
    Ok(serde_json::to_string(&report)?)
}

#[cfg(test)]
mod tests {
    use super::super::wire::{put_bytes, put_num};
    use super::*;
    use serde_json::Value;
    fn values(numbers: &[(u32, u64)], strings: &[(u32, &str)]) -> Vec<u8> {
        let mut p = Vec::new();
        for (id, v) in numbers {
            put_num(&mut p, *id, *v);
        }
        for (id, v) in strings {
            put_bytes(&mut p, *id, v.as_bytes());
        }
        p
    }
    fn packet(trace: &mut Vec<u8>, field: u32, value: &[u8], ts: Option<u64>) {
        let mut p = Vec::new();
        put_bytes(&mut p, field, value);
        if let Some(ts) = ts {
            put_num(&mut p, 8, ts);
        }
        put_bytes(trace, 1, &p);
    }
    fn event(ts: u64, tid: u32, field: u32, payload: Vec<u8>) -> Vec<u8> {
        let mut e = values(&[(1, ts), (2, u64::from(tid))], &[]);
        put_bytes(&mut e, field, &payload);
        e
    }
    fn bundle(trace: &mut Vec<u8>, cpu: u64, events: Vec<Vec<u8>>, lost: bool) {
        let mut b = values(&[(1, cpu), (3, u64::from(lost))], &[]);
        for e in events {
            put_bytes(&mut b, 2, &e);
        }
        packet(trace, 1, &b, None);
    }
    fn switch(ts: u64, prev: u64, state: u64, next: u64) -> Vec<u8> {
        event(
            ts,
            prev as u32,
            4,
            values(
                &[(2, prev), (4, state), (6, next)],
                &[
                    (1, if prev == 11 { "RenderThread" } else { "main" }),
                    (5, if next == 11 { "RenderThread" } else { "main" }),
                ],
            ),
        )
    }
    fn wake(ts: u64, tid: u64) -> Vec<u8> {
        event(
            ts,
            1,
            17,
            values(
                &[(2, tid), (4, 1)],
                &[(1, if tid == 11 { "RenderThread" } else { "main" })],
            ),
        )
    }
    fn marker(ts: u64, tid: u32, s: &str) -> Vec<u8> {
        event(ts, tid, 3, values(&[], &[(2, s)]))
    }
    fn metadata(trace: &mut Vec<u8>) {
        let mut tree = Vec::new();
        put_bytes(
            &mut tree,
            1,
            &values(&[(1, 10)], &[(3, "com.example.test")]),
        );
        put_bytes(
            &mut tree,
            2,
            &values(&[(1, 11), (3, 10)], &[(2, "RenderThread")]),
        );
        packet(trace, 2, &tree, None);
    }
    fn report(trace: &[u8], target: Option<u32>) -> Result<Value> {
        Ok(serde_json::from_str(&analyze(
            trace, target, None, 50, 16.667,
        )?)?)
    }

    #[test]
    fn correlates_five_categories_and_sorts_cross_cpu_events() -> Result<()> {
        let mut trace = Vec::new();
        metadata(&mut trace);
        // Write CPU 1 before CPU 0, as Perfetto buffers are not globally ordered.
        bundle(
            &mut trace,
            1,
            vec![
                wake(10_000_000, 11),
                switch(80_000_000, 0, 1, 11),
                marker(100_000_000, 11, "B|10|DrawFrame"),
                marker(180_000_000, 11, "E"),
                event(
                    200_000_000,
                    11,
                    50,
                    values(&[(1, 99), (3, 10), (5, 0), (7, 0)], &[]),
                ),
            ],
            false,
        );
        bundle(
            &mut trace,
            0,
            vec![
                wake(0, 10),
                switch(60_000_000, 0, 1, 10),
                marker(100_000_000, 10, "B|10|Choreographer#doFrame 42"),
                marker(130_000_000, 10, "E"),
                event(270_000_000, 10, 51, values(&[(1, 99)], &[])),
            ],
            false,
        );
        let frame = values(&[(2, 123), (4, 10), (6, 4), (9, 64 | 1024)], &[]);
        let mut ft = Vec::new();
        put_bytes(&mut ft, 4, &frame);
        packet(&mut trace, 76, &ft, Some(300_000_000));
        let r = report(&trace, Some(10))?;
        assert_eq!(r["main_thread_unscheduled"]["count"], 1);
        assert_eq!(r["render_thread_anomalies"]["count"], 2);
        assert_eq!(r["frame_anomalies"]["count"], 2);
        assert_eq!(r["binder_delivery_delays"]["count"], 1);
        assert_eq!(
            r["binder_delivery_delays"]["examples"][0]["duration_ms"],
            70.0
        );
        assert_eq!(r["cpu_contention"]["count"], 2);
        assert!(r["frame_anomalies"]["examples"][1]["evidence"]
            .as_str()
            .is_some_and(|s| s.contains("dropped=true")));
        assert_eq!(report(&trace, Some(55))?["frame_anomalies"]["count"], 0);
        assert_eq!(
            serde_json::from_str::<Value>(&analyze(
                &trace,
                None,
                Some("com.example.test"),
                50,
                16.667
            )?)?["target_pid"],
            10
        );
        Ok(())
    }
    #[test]
    fn sequence_start_flags_do_not_extend_observed_duration_to_boot() -> Result<()> {
        let mut trace = Vec::new();
        let dropped = values(&[(42, 1)], &[]);
        put_bytes(&mut trace, 1, &dropped);
        bundle(
            &mut trace,
            0,
            vec![wake(900_000_000_000, 10), switch(900_100_000_000, 0, 1, 10)],
            false,
        );
        let r = report(&trace, Some(10))?;
        assert_eq!(r["observed_duration_ms"], 100.0);
        assert_eq!(r["coverage"]["unlocated_loss_markers"], 1);
        assert_eq!(r["status"], "partial");
        Ok(())
    }
    #[test]
    fn internal_event_and_thread_budgets_are_enforced() -> Result<()> {
        let mut trace = Trace {
            events: Vec::new(),
            threads: HashMap::new(),
            processes: HashMap::new(),
            coverage: Coverage::default(),
        };
        trace.events.resize(
            MAX_EVENTS,
            Event {
                ts: 0,
                tid: 0,
                cpu: 0,
                kind: Kind::Loss,
            },
        );
        assert!(trace
            .push(Event {
                ts: 1,
                tid: 0,
                cpu: 0,
                kind: Kind::Loss
            })
            .is_err());
        for id in 0..MAX_THREADS as u32 {
            trace.thread(id)?;
        }
        assert!(trace.thread(MAX_THREADS as u32).is_err());
        Ok(())
    }
    #[test]
    fn sleeping_and_unmatched_intervals_are_not_stalls() -> Result<()> {
        let mut trace = Vec::new();
        metadata(&mut trace);
        bundle(
            &mut trace,
            0,
            vec![
                switch(0, 0, 1, 10),
                switch(1_000_000, 10, 1, 0),
                switch(2_000_000_000, 0, 1, 10),
                marker(2_100_000_000, 10, "E"),
                wake(3_000_000_000, 11),
            ],
            false,
        );
        let r = report(&trace, Some(10))?;
        assert_eq!(r["main_thread_unscheduled"]["count"], 0);
        assert_eq!(r["cpu_contention"]["count"], 0);
        Ok(())
    }
    #[test]
    fn loss_resets_pairs_and_marks_partial() -> Result<()> {
        let mut trace = Vec::new();
        metadata(&mut trace);
        bundle(
            &mut trace,
            0,
            vec![wake(0, 10), event(1, 10, 50, values(&[(1, 7)], &[]))],
            false,
        );
        bundle(
            &mut trace,
            0,
            vec![
                switch(100_000_000, 0, 1, 10),
                event(200_000_000, 11, 51, values(&[(1, 7)], &[])),
            ],
            true,
        );
        let r = report(&trace, Some(10))?;
        assert_eq!(r["status"], "partial");
        assert_eq!(r["main_thread_unscheduled"]["count"], 0);
        assert_eq!(r["binder_delivery_delays"]["count"], 0);
        Ok(())
    }
    #[test]
    fn wakeup_rate_is_observed_and_bounded() -> Result<()> {
        let mut trace = Vec::new();
        metadata(&mut trace);
        let mut e = Vec::new();
        for n in 0..=200 {
            e.push(wake(n * 5_000_000, 10));
        }
        bundle(&mut trace, 0, e, false);
        let r = report(&trace, Some(10))?;
        assert_eq!(r["excessive_wakeups"]["count"], 1);
        assert_eq!(r["cpu_threads"][0]["wakeups"], 201);
        Ok(())
    }
    #[test]
    fn malformed_unknown_compressed_and_clock_encodings() -> Result<()> {
        for input in [&[10, 128][..], &[10, 4, 8][..], &[0][..], &[255; 12][..]] {
            assert!(report(input, None).is_err());
        }
        let mut trace = Vec::new();
        packet(&mut trace, 999, &[1, 2, 3], None);
        packet(&mut trace, 50, &[0], None);
        let r = report(&trace, None)?;
        assert_eq!(r["status"], "unsupported");
        assert_eq!(r["coverage"]["compressed_packets"], 1);
        let mut trace = Vec::new();
        let b = values(&[(5, 1)], &[]);
        packet(&mut trace, 1, &b, None);
        assert_eq!(
            report(&trace, None)?["coverage"]["unsupported_clock_bundles"],
            1
        );
        assert!(analyze(&trace, None, Some("unknown.package"), 50, 16.667).is_err());
        let mut trace = Vec::new();
        put_bytes(&mut trace, 1, &vec![0; MAX_PACKET + 1]);
        assert!(report(&trace, None).is_err());
        Ok(())
    }
}
