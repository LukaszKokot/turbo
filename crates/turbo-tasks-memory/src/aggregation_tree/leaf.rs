use std::{hash::Hash, sync::Arc};

use nohash_hasher::IsEnabled;
use ref_cast::RefCast;

use super::{
    bottom_connection::{BottomConnection, DistanceCountMap},
    bottom_tree::BottomTree,
    inner_refs::{BottomRef, ChildLocation},
    top_tree::TopTree,
    AggregationContext, AggregationItemLock,
};

pub struct AggregationTreeLeaf<T, I: IsEnabled> {
    top_trees: Vec<Option<Arc<TopTree<T>>>>,
    bottom_trees: Vec<Option<Arc<BottomTree<T, I>>>>,
    upper: BottomConnection<T, I>,
}

impl<T, I: Clone + Eq + Hash + IsEnabled> AggregationTreeLeaf<T, I> {
    pub fn new() -> Self {
        Self {
            top_trees: Vec::new(),
            bottom_trees: Vec::new(),
            upper: BottomConnection::new(),
        }
    }

    pub fn add_children_job<'a, C: AggregationContext<Info = T, ItemRef = I>>(
        &self,
        context: &'a C,
        children: Vec<I>,
    ) -> impl FnOnce() + 'a
    where
        I: 'a,
        T: 'a,
    {
        let uppers = self.upper.as_cloned_uppers();
        move || {
            let children = children.iter().map(|child| (context.hash(child), child));
            uppers.add_children_of_child(context, children);
        }
    }

    pub fn add_child_job<'a, C: AggregationContext<Info = T, ItemRef = I>>(
        &self,
        context: &'a C,
        child: &'a I,
    ) -> impl FnOnce() + 'a
    where
        T: 'a,
    {
        let uppers = self.upper.as_cloned_uppers();
        move || {
            let hash = context.hash(child);
            uppers.add_child_of_child(context, child, hash);
        }
    }

    pub fn remove_child<C: AggregationContext<Info = T, ItemRef = I>>(
        &self,
        context: &C,
        child: &I,
    ) {
        self.upper
            .as_cloned_uppers()
            .remove_child_of_child(context, child);
    }

    pub fn change<C: AggregationContext<Info = T, ItemRef = I>>(
        &self,
        context: &C,
        change: &C::ItemChange,
    ) {
        context.on_change(change);
        self.upper.child_change(context, change);
    }

    pub fn change_job<'a, C: AggregationContext<Info = T, ItemRef = I>>(
        &self,
        context: &'a C,
        change: C::ItemChange,
    ) -> impl FnOnce() + 'a
    where
        I: 'a,
        T: 'a,
    {
        let uppers = self.upper.as_cloned_uppers();
        move || {
            context.on_change(&change);
            uppers.child_change(context, &change);
        }
    }

    pub fn get_root_info<C: AggregationContext<Info = T, ItemRef = I>>(
        &self,
        context: &C,
        root_info_type: &C::RootInfoType,
    ) -> C::RootInfo {
        self.upper.get_root_info(
            context,
            root_info_type,
            context.new_root_info(root_info_type),
        )
    }

    pub fn has_upper(&self) -> bool {
        !self.upper.is_unset()
    }
}

fn get_or_create_in_vec<T>(
    vec: &mut Vec<Option<T>>,
    index: usize,
    create: impl FnOnce() -> T,
) -> (&mut T, bool) {
    if vec.len() <= index {
        vec.resize_with(index + 1, || None);
    }
    let item = &mut vec[index];
    if item.is_none() {
        *item = Some(create());
        (item.as_mut().unwrap(), true)
    } else {
        (item.as_mut().unwrap(), false)
    }
}

#[tracing::instrument(skip(context, reference))]
pub fn top_tree<C: AggregationContext>(
    context: &C,
    reference: &C::ItemRef,
    depth: u8,
) -> Arc<TopTree<C::Info>> {
    let new_top_tree = {
        let mut item = context.item(reference);
        let leaf = item.leaf();
        let (tree, new) = get_or_create_in_vec(&mut leaf.top_trees, depth as usize, || {
            Arc::new(TopTree::new(depth))
        });
        if !new {
            return tree.clone();
        }
        tree.clone()
    };
    let bottom_tree = bottom_tree(context, reference, depth + 4);
    bottom_tree.add_top_tree_upper(context, &new_top_tree);
    new_top_tree
}

pub fn bottom_tree<C: AggregationContext>(
    context: &C,
    reference: &C::ItemRef,
    height: u8,
) -> Arc<BottomTree<C::Info, C::ItemRef>> {
    let span;
    let new_bottom_tree;
    let mut result = None;
    {
        let mut item = context.item(reference);
        let leaf = item.leaf();
        let (tree, new) = get_or_create_in_vec(&mut leaf.bottom_trees, height as usize, || {
            Arc::new(BottomTree::new(reference.clone(), height))
        });
        if !new {
            return tree.clone();
        }
        new_bottom_tree = tree.clone();
        span = (height > 1).then(|| tracing::trace_span!("bottom_tree", height).entered());

        if height == 0 {
            result = Some(add_left_upper_to_item_step_1::<C>(
                &mut item,
                &new_bottom_tree,
            ));
        }
    }
    if let Some(result) = result {
        add_left_upper_to_item_step_2(context, reference, &new_bottom_tree, result);
    }
    if height != 0 {
        bottom_tree(context, reference, height - 1)
            .add_left_bottom_tree_upper(context, &new_bottom_tree);
    }
    new_bottom_tree
}

#[must_use]
pub fn add_inner_upper_to_item<C: AggregationContext>(
    context: &C,
    reference: &C::ItemRef,
    upper: &Arc<BottomTree<C::Info, C::ItemRef>>,
    nesting_level: u8,
) -> bool {
    let (change, children) = {
        let mut item = context.item(reference);
        let leaf = item.leaf();
        let BottomConnection::Inner(inner) = &mut leaf.upper else {
            return false;
        };
        let new = inner.add_clonable(BottomRef::ref_cast(upper), nesting_level);
        if new {
            let change = item.get_add_change();
            (
                change,
                item.children().map(|r| r.into_owned()).collect::<Vec<_>>(),
            )
        } else {
            return true;
        }
    };
    if let Some(change) = change {
        context.on_add_change(&change);
        upper.child_change(context, &change);
    }
    if !children.is_empty() {
        upper.add_children_of_child(
            context,
            ChildLocation::Inner,
            children.iter().map(|child| (context.hash(&child), child)),
            nesting_level + 1,
        )
    }
    true
}

#[must_use]
fn add_left_upper_to_item_step_1<C: AggregationContext>(
    item: &mut C::ItemLock<'_>,
    upper: &Arc<BottomTree<C::Info, C::ItemRef>>,
) -> (
    Option<C::ItemChange>,
    Vec<C::ItemRef>,
    DistanceCountMap<BottomRef<C::Info, C::ItemRef>>,
    Option<C::ItemChange>,
    Vec<C::ItemRef>,
) {
    let old_inner = item.leaf().upper.set_left_upper(upper);
    let remove_change_for_old_inner = (!old_inner.is_unset())
        .then(|| item.get_remove_change())
        .flatten();
    let children_for_old_inner = (!old_inner.is_unset())
        .then(|| {
            item.children()
                .map(|child| child.into_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    (
        item.get_add_change(),
        item.children().map(|r| r.into_owned()).collect(),
        old_inner,
        remove_change_for_old_inner,
        children_for_old_inner,
    )
}

fn add_left_upper_to_item_step_2<C: AggregationContext>(
    context: &C,
    reference: &C::ItemRef,
    upper: &Arc<BottomTree<C::Info, C::ItemRef>>,
    step_1_result: (
        Option<C::ItemChange>,
        Vec<C::ItemRef>,
        DistanceCountMap<BottomRef<C::Info, C::ItemRef>>,
        Option<C::ItemChange>,
        Vec<C::ItemRef>,
    ),
) {
    let (change, children, old_inner, remove_change_for_old_inner, following_for_old_uppers) =
        step_1_result;
    if let Some(change) = change {
        context.on_add_change(&change);
        upper.child_change(context, &change);
    }
    if !children.is_empty() {
        upper.add_children_of_child(
            context,
            ChildLocation::Left,
            children.iter().map(|child| (context.hash(&child), child)),
            1,
        )
    }
    for (BottomRef { upper: old_upper }, count) in old_inner.into_counts() {
        old_upper.migrate_old_inner(
            context,
            reference,
            count,
            &remove_change_for_old_inner,
            &following_for_old_uppers,
        );
    }
}

pub fn remove_left_upper_from_item<C: AggregationContext>(
    context: &C,
    reference: &C::ItemRef,
    upper: &Arc<BottomTree<C::Info, C::ItemRef>>,
) {
    let mut item = context.item(reference);
    let leaf = &mut item.leaf();
    leaf.upper.unset_left_upper(upper);
    let change = item.get_remove_change();
    let children = item.children().map(|r| r.into_owned()).collect::<Vec<_>>();
    drop(item);
    if let Some(change) = change {
        context.on_remove_change(&change);
        upper.child_change(context, &change);
    }
    for child in children {
        upper.remove_child_of_child(context, &child)
    }
}

#[must_use]
pub fn remove_inner_upper_from_item<C: AggregationContext>(
    context: &C,
    reference: &C::ItemRef,
    upper: &Arc<BottomTree<C::Info, C::ItemRef>>,
) -> bool {
    let mut item = context.item(reference);
    let BottomConnection::Inner(inner) = &mut item.leaf().upper else {
        return false;
    };
    if !inner.remove_clonable(BottomRef::ref_cast(upper)) {
        // Nothing to do
        return true;
    }
    let change = item.get_remove_change();
    let children = item.children().map(|r| r.into_owned()).collect::<Vec<_>>();
    drop(item);

    if let Some(change) = change {
        context.on_remove_change(&change);
        upper.child_change(context, &change);
    }
    for child in children {
        upper.remove_child_of_child(context, &child)
    }
    true
}
