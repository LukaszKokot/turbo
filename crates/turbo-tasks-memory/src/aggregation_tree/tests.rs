use std::{
    borrow::Cow,
    hash::Hash,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use nohash_hasher::IsEnabled;
use parking_lot::{Mutex, MutexGuard};

use super::{aggregation_info, AggregationContext, AggregationItemLock, AggregationTreeLeaf};
use crate::aggregation_tree::bottom_tree::print_graph;

struct Node {
    inner: Mutex<NodeInner>,
}

impl Node {
    fn incr(&self, context: &NodeAggregationContext) {
        let mut guard = self.inner.lock();
        guard.value += 10000;
        guard
            .aggregation_leaf
            .change(context, &Change { value: 10000 });
    }
}

#[derive(Copy, Clone)]
struct Change {
    value: i32,
}

impl Change {
    fn is_empty(&self) -> bool {
        self.value == 0
    }
}

struct NodeInner {
    children: Vec<Arc<Node>>,
    aggregation_leaf: AggregationTreeLeaf<Aggregated, NodeRef>,
    value: u32,
}

struct NodeAggregationContext<'a> {
    additions: AtomicU32,
    #[allow(dead_code)]
    something_with_lifetime: &'a u32,
    add_value: bool,
}

#[derive(Clone)]
struct NodeRef(Arc<Node>);

impl Hash for NodeRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl IsEnabled for NodeRef {}

impl PartialEq for NodeRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for NodeRef {}

struct NodeGuard {
    guard: MutexGuard<'static, NodeInner>,
    node: Arc<Node>,
}

impl AggregationItemLock for NodeGuard {
    type Info = Aggregated;
    type ItemRef = NodeRef;
    type ItemChange = Change;
    type ChildrenIter<'a> = impl Iterator<Item = Cow<'a, NodeRef>> + 'a;

    fn leaf(&mut self) -> &mut AggregationTreeLeaf<Aggregated, NodeRef> {
        &mut self.guard.aggregation_leaf
    }

    fn children(&self) -> Self::ChildrenIter<'_> {
        self.guard
            .children
            .iter()
            .map(|child| Cow::Owned(NodeRef(child.clone())))
    }

    fn get_remove_change(&self) -> Option<Change> {
        let change = Change {
            value: -(self.guard.value as i32),
        };
        if change.is_empty() {
            None
        } else {
            Some(change)
        }
    }

    fn get_add_change(&self) -> Option<Change> {
        let change = Change {
            value: self.guard.value as i32,
        };
        if change.is_empty() {
            None
        } else {
            Some(change)
        }
    }
}

impl<'a> AggregationContext for NodeAggregationContext<'a> {
    type ItemLock<'l> = NodeGuard where Self: 'l;
    type Info = Aggregated;
    type ItemRef = NodeRef;
    type ItemChange = Change;

    fn item<'b>(&'b self, reference: &Self::ItemRef) -> Self::ItemLock<'b> {
        let r = reference.0.clone();
        let guard = r.inner.lock();
        NodeGuard {
            guard: unsafe { std::mem::transmute(guard) },
            node: r,
        }
    }

    fn apply_change(&self, info: &mut Aggregated, change: &Change) -> Option<Change> {
        if info.value != 0 {
            self.additions.fetch_add(1, Ordering::SeqCst);
        }
        if self.add_value {
            info.value += change.value;
        }
        Some(change.clone())
    }

    fn info_to_add_change(&self, info: &Self::Info) -> Option<Self::ItemChange> {
        let change = Change {
            value: info.value as i32,
        };
        if change.is_empty() {
            None
        } else {
            Some(change)
        }
    }

    fn info_to_remove_change(&self, info: &Self::Info) -> Option<Self::ItemChange> {
        let change = Change {
            value: -(info.value as i32),
        };
        if change.is_empty() {
            None
        } else {
            Some(change)
        }
    }

    type RootInfo = bool;

    type RootInfoType = ();

    fn new_root_info(&self, root_info_type: &Self::RootInfoType) -> Self::RootInfo {
        match root_info_type {
            () => false,
        }
    }

    fn info_to_root_info(
        &self,
        info: &Self::Info,
        root_info_type: &Self::RootInfoType,
    ) -> Self::RootInfo {
        match root_info_type {
            () => info.active,
        }
    }

    fn merge_root_info(
        &self,
        root_info: &mut Self::RootInfo,
        other: Self::RootInfo,
    ) -> std::ops::ControlFlow<()> {
        if other {
            *root_info = true;
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    }
}

#[derive(Default)]
struct Aggregated {
    value: i32,
    active: bool,
}

#[test]
fn chain() {
    let something_with_lifetime = 0;
    let context = NodeAggregationContext {
        additions: AtomicU32::new(0),
        something_with_lifetime: &something_with_lifetime,
        add_value: true,
    };
    let leaf = Arc::new(Node {
        inner: Mutex::new(NodeInner {
            children: vec![],
            aggregation_leaf: AggregationTreeLeaf::new(),
            value: 10000,
        }),
    });
    let mut current = leaf.clone();
    for i in 1..=100 {
        current = Arc::new(Node {
            inner: Mutex::new(NodeInner {
                children: vec![current],
                aggregation_leaf: AggregationTreeLeaf::new(),
                value: i,
            }),
        });
    }
    let current = NodeRef(current);

    {
        let root_info = leaf
            .inner
            .lock()
            .aggregation_leaf
            .get_root_info(&context, &());
        assert_eq!(root_info, false);
    }

    {
        let aggregated = aggregation_info(&context, &current);
        assert_eq!(aggregated.lock().value, 15050);
    }
    assert_eq!(context.additions.load(Ordering::SeqCst), 100);
    context.additions.store(0, Ordering::SeqCst);

    print(&context, &current);

    {
        let root_info = leaf
            .inner
            .lock()
            .aggregation_leaf
            .get_root_info(&context, &());
        assert_eq!(root_info, false);
    }

    leaf.incr(&context);
    // The change need to propagate through 5 top trees and 5 bottom trees
    assert_eq!(context.additions.load(Ordering::SeqCst), 6);
    context.additions.store(0, Ordering::SeqCst);

    {
        let aggregated = aggregation_info(&context, &current);
        let mut aggregated = aggregated.lock();
        assert_eq!(aggregated.value, 25050);
        (*aggregated).active = true;
    }
    assert_eq!(context.additions.load(Ordering::SeqCst), 0);
    context.additions.store(0, Ordering::SeqCst);

    {
        let root_info = leaf
            .inner
            .lock()
            .aggregation_leaf
            .get_root_info(&context, &());
        assert_eq!(root_info, true);
    }

    let i = 101;
    let current = Arc::new(Node {
        inner: Mutex::new(NodeInner {
            children: vec![current.0],
            aggregation_leaf: AggregationTreeLeaf::new(),
            value: i,
        }),
    });
    let current = NodeRef(current);

    {
        let aggregated = aggregation_info(&context, &current);
        let aggregated = aggregated.lock();
        assert_eq!(aggregated.value, 25151);
    }
    // This should be way less the 100 to prove that we are reusing trees
    assert_eq!(context.additions.load(Ordering::SeqCst), 1);
    context.additions.store(0, Ordering::SeqCst);

    leaf.incr(&context);
    // This should be less the 20 to prove that we are reusing trees
    assert_eq!(context.additions.load(Ordering::SeqCst), 9);
    context.additions.store(0, Ordering::SeqCst);

    {
        let root_info = leaf
            .inner
            .lock()
            .aggregation_leaf
            .get_root_info(&context, &());
        assert_eq!(root_info, true);
    }
}

#[test]
fn chain_double_connected() {
    let something_with_lifetime = 0;
    let context = NodeAggregationContext {
        additions: AtomicU32::new(0),
        something_with_lifetime: &something_with_lifetime,
        add_value: true,
    };
    let leaf = Arc::new(Node {
        inner: Mutex::new(NodeInner {
            children: vec![],
            aggregation_leaf: AggregationTreeLeaf::new(),
            value: 1,
        }),
    });
    let mut current = leaf.clone();
    let mut current2 = Arc::new(Node {
        inner: Mutex::new(NodeInner {
            children: vec![leaf.clone()],
            aggregation_leaf: AggregationTreeLeaf::new(),
            value: 2,
        }),
    });
    for i in 3..=100 {
        let new_node = Arc::new(Node {
            inner: Mutex::new(NodeInner {
                children: vec![current, current2.clone()],
                aggregation_leaf: AggregationTreeLeaf::new(),
                value: i,
            }),
        });
        current = current2;
        current2 = new_node;
    }
    let current = NodeRef(current2);

    print(&context, &current);

    {
        let aggregated = aggregation_info(&context, &current);
        assert_eq!(aggregated.lock().value, 7627);
    }
    assert_eq!(context.additions.load(Ordering::SeqCst), 184);
    context.additions.store(0, Ordering::SeqCst);
}

const RECT_SIZE: usize = 100;
const RECT_MULT: usize = 100;

#[test]
fn rectangle_tree() {
    let something_with_lifetime = 0;
    let context = NodeAggregationContext {
        additions: AtomicU32::new(0),
        something_with_lifetime: &something_with_lifetime,
        add_value: false,
    };
    let mut nodes: Vec<Vec<Arc<Node>>> = Vec::new();
    for y in 0..RECT_SIZE {
        let mut line: Vec<Arc<Node>> = Vec::new();
        for x in 0..RECT_SIZE {
            let mut children = Vec::new();
            if x > 0 {
                children.push(line[x - 1].clone());
            }
            if y > 0 {
                children.push(nodes[y - 1][x].clone());
            }
            let value = (x + y * RECT_MULT) as u32;
            let node = Arc::new(Node {
                inner: Mutex::new(NodeInner {
                    children,
                    aggregation_leaf: AggregationTreeLeaf::new(),
                    value,
                }),
            });
            line.push(node.clone());
        }
        nodes.push(line);
    }

    let root = NodeRef(nodes[RECT_SIZE - 1][RECT_SIZE - 1].clone());

    print(&context, &root);
}

#[test]
fn rectangle_adding_tree() {
    let something_with_lifetime = 0;
    let context = NodeAggregationContext {
        additions: AtomicU32::new(0),
        something_with_lifetime: &something_with_lifetime,
        add_value: false,
    };
    let mut nodes: Vec<Vec<Arc<Node>>> = Vec::new();

    fn add_child(parent: &Arc<Node>, node: &Arc<Node>, context: &NodeAggregationContext<'_>) {
        let node_ref = NodeRef(node.clone());
        let mut state = parent.inner.lock();
        state.children.push(node.clone());
        let job = state.aggregation_leaf.add_child_job(context, &node_ref);
        drop(state);
        job();
    }
    for y in 0..RECT_SIZE {
        let mut line: Vec<Arc<Node>> = Vec::new();
        for x in 0..RECT_SIZE {
            let value = (x + y * RECT_MULT) as u32;
            let node = Arc::new(Node {
                inner: Mutex::new(NodeInner {
                    children: Vec::new(),
                    aggregation_leaf: AggregationTreeLeaf::new(),
                    value,
                }),
            });
            line.push(node.clone());
            if x > 0 {
                let parent = &line[x - 1];
                add_child(parent, &node, &context);
            }
            if y > 0 {
                let parent = &nodes[y - 1][x];
                add_child(parent, &node, &context);
            }
            if x == 0 && y == 0 {
                aggregation_info(&context, &NodeRef(node.clone()))
                    .lock()
                    .active = true;
            }
        }
        nodes.push(line);
    }

    let root = NodeRef(nodes[0][0].clone());

    print(&context, &root);
}

fn print(context: &NodeAggregationContext<'_>, current: &NodeRef) {
    println!("digraph {{");
    let start = 0;
    let end = 3;
    for i in start..end {
        print_graph(context, current, i, false, |item| {
            format!("{}", item.0.inner.lock().value)
        });
    }
    for i in start + 1..end + 1 {
        print_graph(context, current, i, true, |item| {
            format!("{}", item.0.inner.lock().value)
        });
    }
    println!("\n}}");
}
