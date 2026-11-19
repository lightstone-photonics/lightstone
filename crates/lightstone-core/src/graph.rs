//! Component graph: configuration format, validation, evaluation and per-node caching
//! (docs/architecture.md, Section 7.3).
//!
//! ```json
//! { "settings": { "baud": 53.125e9, "sps": 16, "resolution": "mode_group" },
//!   "nodes": [ { "id": "src", "kind": "prbs13q" },
//!              { "id": "drv", "kind": "driver" }, … ],
//!   "wires": [ { "from": "src.out", "to": "drv.in" }, … ] }
//! ```
//!
//! Rules: wires connect ports of matching type; every input is connected exactly once; the
//! graph is acyclic; an optical output may feed at most one non-monitor input (use a splitter
//! or probe otherwise). Data and electrical outputs may fan out freely.

use crate::component::*;
use crate::components::register_builtins;
use crate::signal::{Settings, TimeGrid};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireConfig {
    /// `"node.port"` of an output.
    pub from: String,
    /// `"node.port"` of an input.
    pub to: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphConfig {
    pub settings: Settings,
    pub nodes: Vec<NodeConfig>,
    pub wires: Vec<WireConfig>,
    /// Editor layout (e.g. node positions), kept with the graph but ignored by the engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<serde_json::Value>,
}

impl GraphConfig {
    /// Replace renamed kinds by their current names ([`RENAMED_KINDS`]).
    pub fn normalize_kinds(&mut self) {
        for n in self.nodes.iter_mut() {
            if canonical_kind(&n.kind) != n.kind {
                n.kind = canonical_kind(&n.kind).to_string();
            }
        }
    }

    pub fn node(&mut self, id: &str, kind: &str, params: serde_json::Value) -> &mut Self {
        self.nodes.push(NodeConfig {
            id: id.into(),
            kind: kind.into(),
            params,
        });
        self
    }

    pub fn wire(&mut self, from: &str, to: &str) -> &mut Self {
        self.wires.push(WireConfig {
            from: from.into(),
            to: to.into(),
        });
        self
    }
}

/// Information available when a component is created.
pub struct NodeInfo<'a> {
    pub id: &'a str,
    pub settings: &'a Settings,
}

pub type Factory = Arc<dyn Fn(&serde_json::Value, &NodeInfo) -> Result<Box<dyn Component>, String>>;

/// Component kinds known to the engine.
#[derive(Clone)]
pub struct Registry {
    factories: HashMap<String, Factory>,
    /// Built-in kinds (core and analysis nodes); custom components cannot replace them.
    builtin: std::collections::HashSet<String>,
}

impl Registry {
    pub fn empty() -> Self {
        Self {
            factories: HashMap::new(),
            builtin: Default::default(),
        }
    }

    /// Register a built-in kind (cannot be replaced by custom components).
    pub fn register_builtin<F>(&mut self, kind: &str, f: F)
    where
        F: Fn(&serde_json::Value, &NodeInfo) -> Result<Box<dyn Component>, String> + 'static,
    {
        self.register(kind, f);
        self.builtin.insert(kind.into());
    }

    pub fn is_builtin(&self, kind: &str) -> bool {
        self.builtin.contains(kind)
    }

    /// Built-in kinds, sorted.
    pub fn builtin_kinds(&self) -> Vec<String> {
        let mut k: Vec<String> = self.builtin.iter().cloned().collect();
        k.sort();
        k
    }

    pub fn builtin() -> Self {
        let mut r = Self::empty();
        register_builtins(&mut r);
        r
    }

    pub fn register<F>(&mut self, kind: &str, f: F)
    where
        F: Fn(&serde_json::Value, &NodeInfo) -> Result<Box<dyn Component>, String> + 'static,
    {
        self.factories.insert(kind.into(), Arc::new(f));
    }

    pub fn kinds(&self) -> Vec<String> {
        let mut k: Vec<String> = self.factories.keys().cloned().collect();
        k.sort();
        k
    }

    pub fn create(
        &self,
        kind: &str,
        params: &serde_json::Value,
        info: &NodeInfo,
    ) -> Result<Box<dyn Component>, String> {
        let f = self
            .factories
            .get(canonical_kind(kind))
            .ok_or_else(|| format!("unknown component kind '{kind}'"))?;
        f(params, info)
    }
}

/// Kinds that were renamed: (old name, current name). Graphs with an old name still load;
/// the engine replaces it by the current one.
pub const RENAMED_KINDS: &[(&str, &str)] =
    &[("decision", "receiver_output"), ("fiber", "custom_fiber")];

/// Current name of a component kind (resolves [`RENAMED_KINDS`]).
pub fn canonical_kind(kind: &str) -> &str {
    RENAMED_KINDS
        .iter()
        .find(|(old, _)| *old == kind)
        .map_or(kind, |(_, new)| new)
}

/// A graph validation or evaluation problem.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GraphIssue {
    pub node: Option<String>,
    /// Index into `GraphConfig::wires`.
    pub wire: Option<usize>,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct GraphError(pub Vec<GraphIssue>);

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msgs: Vec<String> = self
            .0
            .iter()
            .map(|i| match (&i.node, i.wire) {
                (Some(n), _) => format!("{n}: {}", i.message),
                (None, Some(w)) => format!("wire {}: {}", w + 1, i.message),
                (None, None) => i.message.clone(),
            })
            .collect();
        write!(f, "{}", msgs.join("; "))
    }
}

impl std::error::Error for GraphError {}

/// Port description for editors and bindings.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PortInfo {
    pub name: String,
    /// `data`, `electrical`, `electrical (current)`, …, `optical`.
    #[serde(rename = "type")]
    pub ty: String,
    pub monitor: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NodePorts {
    pub id: String,
    pub kind: String,
    pub linear: bool,
    pub inputs: Vec<PortInfo>,
    pub outputs: Vec<PortInfo>,
}

struct Entry {
    key: u64,
    output_names: Vec<String>,
    outputs: Vec<Signal>,
    report: Option<Report>,
    warnings: Vec<String>,
}

struct Plan {
    order: Vec<usize>,
    comps: Vec<Box<dyn Component>>,
    /// Per node, per input: (source node, source output index).
    sources: Vec<Vec<(usize, usize)>>,
    output_names: Vec<Vec<String>>,
}

/// Evaluates a [`GraphConfig`], caching node outputs across runs.
pub struct GraphEngine {
    graph: GraphConfig,
    registry: Registry,
    cache: HashMap<String, Entry>,
    shared: SharedCache,
    computed_last_run: usize,
}

fn split_ref(s: &str) -> Option<(&str, &str)> {
    s.split_once('.')
}

impl GraphEngine {
    pub fn new(graph: GraphConfig) -> Self {
        Self::with_registry(graph, Registry::builtin())
    }

    pub fn with_registry(mut graph: GraphConfig, registry: Registry) -> Self {
        graph.normalize_kinds();
        Self {
            graph,
            registry,
            cache: HashMap::new(),
            shared: SharedCache::default(),
            computed_last_run: 0,
        }
    }

    pub fn graph(&self) -> &GraphConfig {
        &self.graph
    }

    /// Replace the graph; cached results of unchanged nodes are reused.
    pub fn set_graph(&mut self, mut graph: GraphConfig) {
        graph.normalize_kinds();
        self.graph = graph;
    }

    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    /// Drop all cached node outputs (e.g. after re-registering a custom component kind, whose
    /// behavior is not part of the cache key).
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Ports of every node that can be created (nodes with invalid kinds or parameters are
    /// left out).
    pub fn node_ports(&self) -> Vec<NodePorts> {
        let info = |p: &PortSpec| PortInfo {
            name: p.name.clone(),
            ty: p.ty.name(),
            monitor: p.monitor,
        };
        self.graph
            .nodes
            .iter()
            .filter_map(|n| {
                let c = self
                    .registry
                    .create(
                        &n.kind,
                        &n.params,
                        &NodeInfo {
                            id: &n.id,
                            settings: &self.graph.settings,
                        },
                    )
                    .ok()?;
                Some(NodePorts {
                    id: n.id.clone(),
                    kind: n.kind.clone(),
                    linear: c.linearity() == Linearity::Linear,
                    inputs: c.inputs().iter().map(info).collect(),
                    outputs: c.outputs().iter().map(info).collect(),
                })
            })
            .collect()
    }

    pub fn grid(&self) -> TimeGrid {
        TimeGrid::new(&self.graph.settings)
    }

    pub fn spectral(&mut self) -> crate::dsp::Spectral {
        let n = self.grid().n();
        self.shared.spectral(n)
    }

    /// Number of nodes recomputed (not taken from the cache) in the last run.
    pub fn computed_last_run(&self) -> usize {
        self.computed_last_run
    }

    fn plan(&self) -> Result<Plan, Vec<GraphIssue>> {
        let g = &self.graph;
        let mut issues = Vec::new();
        let issue = |node: Option<&str>, wire: Option<usize>, m: String| GraphIssue {
            node: node.map(String::from),
            wire,
            message: m,
        };
        // Nodes.
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut comps: Vec<Option<Box<dyn Component>>> = Vec::new();
        for (i, n) in g.nodes.iter().enumerate() {
            if n.id.is_empty() || n.id.contains('.') {
                issues.push(issue(
                    Some(&n.id),
                    None,
                    "node ids must be non-empty and must not contain '.'".into(),
                ));
            }
            if index.insert(&n.id, i).is_some() {
                issues.push(issue(Some(&n.id), None, "duplicate node id".into()));
            }
            let info = NodeInfo {
                id: &n.id,
                settings: &g.settings,
            };
            match self.registry.create(&n.kind, &n.params, &info) {
                Ok(c) => comps.push(Some(c)),
                Err(e) => {
                    issues.push(issue(Some(&n.id), None, e));
                    comps.push(None);
                }
            }
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        let comps: Vec<Box<dyn Component>> = comps.into_iter().map(|c| c.unwrap()).collect();
        let ins: Vec<Vec<PortSpec>> = comps.iter().map(|c| c.inputs()).collect();
        let outs: Vec<Vec<PortSpec>> = comps.iter().map(|c| c.outputs()).collect();

        // Wires.
        let mut sources: Vec<Vec<Option<(usize, usize)>>> =
            ins.iter().map(|v| vec![None; v.len()]).collect();
        let mut consumers: HashMap<(usize, usize), Vec<(usize, usize)>> = HashMap::new();
        for (wi, w) in g.wires.iter().enumerate() {
            let (Some((fnode, fport)), Some((tnode, tport))) =
                (split_ref(&w.from), split_ref(&w.to))
            else {
                issues.push(issue(
                    None,
                    Some(wi),
                    "wire ends must be written as \"node.port\"".into(),
                ));
                continue;
            };
            let (Some(&fi), Some(&ti)) = (index.get(fnode), index.get(tnode)) else {
                issues.push(issue(
                    None,
                    Some(wi),
                    format!("unknown node in wire {} → {}", w.from, w.to),
                ));
                continue;
            };
            let Some(fo) = outs[fi].iter().position(|p| p.name == fport) else {
                issues.push(issue(
                    Some(fnode),
                    Some(wi),
                    format!("no output port '{fport}'"),
                ));
                continue;
            };
            let Some(tin) = ins[ti].iter().position(|p| p.name == tport) else {
                issues.push(issue(
                    Some(tnode),
                    Some(wi),
                    format!("no input port '{tport}'"),
                ));
                continue;
            };
            let (ft, tt) = (outs[fi][fo].ty, ins[ti][tin].ty);
            if !ft.connects_to(&tt) {
                issues.push(issue(
                    Some(tnode),
                    Some(wi),
                    format!(
                        "cannot connect {} output {} to {} input {}",
                        ft.name(),
                        w.from,
                        tt.name(),
                        w.to
                    ),
                ));
                continue;
            }
            if sources[ti][tin].is_some() {
                issues.push(issue(
                    Some(tnode),
                    Some(wi),
                    format!("input '{tport}' is connected more than once"),
                ));
                continue;
            }
            sources[ti][tin] = Some((fi, fo));
            consumers.entry((fi, fo)).or_default().push((ti, tin));
        }
        for (i, s) in sources.iter().enumerate() {
            for (k, src) in s.iter().enumerate() {
                if src.is_none() {
                    issues.push(issue(
                        Some(&g.nodes[i].id),
                        None,
                        format!("input '{}' is not connected", ins[i][k].name),
                    ));
                }
            }
        }
        // Optical fan-out: at most one non-monitor consumer.
        for ((fi, fo), cons) in &consumers {
            if outs[*fi][*fo].ty == PortType::Optical {
                let active = cons
                    .iter()
                    .filter(|(ti, tin)| !ins[*ti][*tin].monitor)
                    .count();
                if active > 1 {
                    issues.push(issue(
                        Some(&g.nodes[*fi].id),
                        None,
                        format!(
                            "optical output '{}' feeds {active} components; light cannot be copied. \
                             Insert a splitter (or a probe for analyzers).",
                            outs[*fi][*fo].name
                        ),
                    ));
                }
            }
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        let sources: Vec<Vec<(usize, usize)>> = sources
            .into_iter()
            .map(|v| v.into_iter().map(|x| x.unwrap()).collect())
            .collect();
        // Topological order (Kahn), stable in node order.
        let n = comps.len();
        let mut indeg: Vec<usize> = sources.iter().map(Vec::len).collect();
        let mut succ: Vec<Vec<usize>> = vec![vec![]; n];
        for (ti, s) in sources.iter().enumerate() {
            for (fi, _) in s {
                succ[*fi].push(ti);
            }
        }
        let mut ready: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
        ready.reverse();
        let mut order = Vec::with_capacity(n);
        while let Some(i) = ready.pop() {
            order.push(i);
            for &t in succ[i].iter().rev() {
                indeg[t] -= 1;
                if indeg[t] == 0 {
                    ready.push(t);
                }
            }
        }
        if order.len() != n {
            let looped: Vec<&str> = (0..n)
                .filter(|i| !order.contains(i))
                .map(|i| g.nodes[i].id.as_str())
                .collect();
            return Err(vec![issue(
                None,
                None,
                format!(
                    "the graph contains a loop ({}); loops are not supported in the outer graph",
                    looped.join(", ")
                ),
            )]);
        }
        Ok(Plan {
            order,
            comps,
            sources,
            output_names: outs
                .iter()
                .map(|v| v.iter().map(|p| p.name.clone()).collect())
                .collect(),
        })
    }

    /// Structural problems of the current graph (empty if it can run).
    pub fn validate(&self) -> Vec<GraphIssue> {
        self.plan().err().unwrap_or_default()
    }

    /// Evaluate the graph. Nodes whose parameters and inputs are unchanged are not recomputed.
    pub fn run(&mut self) -> Result<(), GraphError> {
        let plan = self.plan().map_err(GraphError)?;
        let settings = self.graph.settings.clone();
        let settings_key = serde_json::to_string(&settings).unwrap_or_default();
        let grid = TimeGrid::new(&settings);
        let spectral = self.shared.spectral(grid.n());
        let mut keys = vec![0u64; plan.comps.len()];
        let mut computed = 0;
        for &i in &plan.order {
            let node = &self.graph.nodes[i];
            let mut h = DefaultHasher::new();
            node.kind.hash(&mut h);
            serde_json::to_string(&node.params)
                .unwrap_or_default()
                .hash(&mut h);
            settings_key.hash(&mut h);
            for (fi, fo) in &plan.sources[i] {
                keys[*fi].hash(&mut h);
                fo.hash(&mut h);
            }
            let key = h.finish();
            keys[i] = key;
            if self.cache.get(&node.id).is_some_and(|e| e.key == key) {
                continue;
            }
            let inputs: Vec<Signal> = plan.sources[i]
                .iter()
                .map(|(fi, fo)| self.cache[&self.graph.nodes[*fi].id].outputs[*fo].clone())
                .collect();
            let mut ctx = ProcessCtx {
                node: &node.id,
                settings: &settings,
                grid,
                spectral: spectral.clone(),
                shared: &mut self.shared,
            };
            let comp = &plan.comps[i];
            let node_err = |e: String| {
                GraphError(vec![GraphIssue {
                    node: Some(node.id.clone()),
                    wire: None,
                    message: e,
                }])
            };
            // Linear components with a declared H(f) are applied by the engine.
            let transfer = match comp.linearity() {
                Linearity::Linear => comp.transfer(&mut ctx),
                Linearity::Nonlinear => None,
            };
            let out = match transfer {
                Some(t) => {
                    let t = t.map_err(node_err)?;
                    let outs = comp.outputs();
                    if inputs.len() != 1 || outs.len() != 1 {
                        return Err(node_err(
                            "a component with a transfer function needs exactly one input \
                             and one output"
                                .into(),
                        ));
                    }
                    let mut o = Output::signals(vec![
                        apply_transfer(&inputs[0], &t.h, &outs[0]).map_err(node_err)?
                    ]);
                    o.warnings = t.warnings;
                    o
                }
                None => comp.process(&mut ctx, &inputs).map_err(node_err)?,
            };
            if out.signals.len() != plan.output_names[i].len() {
                return Err(GraphError(vec![GraphIssue {
                    node: Some(node.id.clone()),
                    wire: None,
                    message: "component returned the wrong number of outputs".into(),
                }]));
            }
            computed += 1;
            self.cache.insert(
                node.id.clone(),
                Entry {
                    key,
                    output_names: plan.output_names[i].clone(),
                    outputs: out.signals,
                    report: out.report,
                    warnings: out.warnings,
                },
            );
        }
        let ids: std::collections::HashSet<&String> =
            self.graph.nodes.iter().map(|n| &n.id).collect();
        self.cache.retain(|k, _| ids.contains(k));
        self.computed_last_run = computed;
        Ok(())
    }

    /// Output signal of `node.port` from the last run.
    pub fn output(&self, node: &str, port: &str) -> Option<&Signal> {
        let e = self.cache.get(node)?;
        let i = e.output_names.iter().position(|p| p == port)?;
        e.outputs.get(i)
    }

    /// All output signals of `node` from the last run, as (port name, signal).
    pub fn outputs(&self, node: &str) -> Vec<(&str, &Signal)> {
        self.cache.get(node).map_or_else(Vec::new, |e| {
            e.output_names
                .iter()
                .map(String::as_str)
                .zip(&e.outputs)
                .collect()
        })
    }

    /// Cache key of `node`'s last result: it changes whenever the node or anything upstream
    /// of it changes, so derived values can be cached against it.
    pub fn result_key(&self, node: &str) -> Option<u64> {
        self.cache.get(node).map(|e| e.key)
    }

    /// Report of a node from the last run.
    pub fn report(&self, node: &str) -> Option<&Report> {
        self.cache.get(node)?.report.as_ref()
    }

    /// Warnings of all nodes from the last run, as (node, message).
    pub fn warnings(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .graph
            .nodes
            .iter()
            .filter_map(|n| self.cache.get(&n.id).map(|e| (n, e)))
            .flat_map(|(n, e)| e.warnings.iter().map(move |w| (n.id.clone(), w.clone())))
            .collect();
        v.dedup();
        v
    }
}
