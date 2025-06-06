use std::{collections::BTreeMap, ops::Bound};

use super::{rect::SplitRect, Rect};

#[derive(Clone, Debug, PartialEq)]
pub struct ZSurface {
    pub z_index: i32,
    pub reference: String,
    pub area: Rect
}

impl ZSurface {
    pub fn new(z_index: i32, reference: impl Into<String>, area: Rect) -> Self {
        Self {
            z_index,
            reference: reference.into(),
            area
        }
    }
}

#[derive(Debug)]
struct ZLeaf {
    reference: String,
    area: SplitRect,
}

impl ZLeaf {
    fn new(reference: String, area: Rect) -> Self {
        let area = SplitRect::from(area);
        Self { reference, area }
    }

    fn mask(self, other: &Self) -> Option<Self> {
        let Self { reference, area } = self;

        let area = area.mask_with(&other.area.bounds().expect("Empty ZLeaf should not happen"));

        if area.is_empty() {
            None
        } else {
            Some(Self { reference, area })
        }
    }

    fn mask_in_place(&mut self, other: &Self) -> bool {
        let area = std::mem::take(&mut self.area);
        let mut new_area = area.mask_with(&other.area.bounds().expect("Empty ZLeaf should not happen"));
        std::mem::swap(&mut self.area, &mut new_area);

        !self.area.is_empty()
    }

}

#[derive(Debug, Default)]
struct ZNode {
    leaves: Vec<ZLeaf>
}

impl ZNode {
    fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    fn mask_all_in_place(&mut self, mask: &ZLeaf) -> bool {
        self.leaves.retain_mut(|l| l.mask_in_place(mask));

        !self.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct ZTree {
    nodes: BTreeMap<i32, ZNode>,
}

impl ZTree {
    pub fn new() -> Self { Default::default() }

    /// Insert a new area to the ZTree, at a giver Z-Layer
    ///
    /// This function starts by masking off part of the newly added leaf with upper layers.
    /// If part of the node is still visible, we rebuild the lower layer of the tree, pruning them
    /// while we do so by using the new-node as a mask. Once that done, we re-assemble the tree.
    ///
    /// NOTE: This might not be the most efficient way to go. For a start, assuming the nodes are
    /// inserted in order, we don't ever need to check upper layers (but the current code) should
    /// do nothing in that case, so it should be ok.
    /// On the other hand, rebuilding the lower-layers every iteration might not be good idea, but
    /// I don't want to bother with performance benchmark just yet.
    pub fn insert(&mut self, surface: ZSurface) -> bool {
        let ZSurface {z_index, reference, area} = surface;
        let Some(new_leaf) = self.nodes
            .range((Bound::Excluded(z_index), Bound::Unbounded))
            .flat_map(|(_, n)| &n.leaves)
            .try_fold(ZLeaf::new(reference, area), |new_leaf, upper| new_leaf.mask(upper))
            else { return false };

        // We split the tree because we may end up removing lower nodes altogether
        let mut upper = self.nodes.split_off(&z_index);

        let mut lower = std::mem::take(&mut self.nodes).into_iter()
            .filter_map(|(k, mut n)| {
                if n.mask_all_in_place(&new_leaf) {
                    Some((k, n))
                } else { None }
            }).collect::<BTreeMap<_, _>>();

        upper.entry(z_index).or_default()
            .leaves
            .push(new_leaf);

        lower.append(&mut upper);
        std::mem::swap(
            &mut self.nodes,
            &mut lower
        );

        true
    }

    /// Flatten the ZTree into a vector of ZSurfaces
    pub fn flatten(self) -> Vec<ZSurface> {
        self.into()
    }
}

impl From<ZTree> for Vec<ZSurface> {
    fn from(value: ZTree) -> Self {
        value.nodes.into_iter()
            .flat_map(|(z_index, n)| n.leaves.into_iter()
                .filter_map(move |l| l.area.bounds()
                    .map(|area| ZSurface {
                        z_index,
                        reference: l.reference,
                        area
                    })
                )
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanity_empty_tree() {
        let empty_tree = ZTree::new();

        let expected: Vec<ZSurface> = Default::default();
        let result: Vec<ZSurface> = empty_tree.into();

        assert_eq!(expected, result)
    }

    #[test]
    fn sanity_one_surface() {
        let mut tree = ZTree::new();
        let s = ZSurface::new(0, "test_surface", Rect::new(0, 0, 100, 100));

        assert!(tree.insert(s.clone()));

        let expected = vec![s];

        assert_eq!(expected, tree.flatten())
    }

    #[test]
    fn sanity_one_layer() {
        let mut tree = ZTree::new();

        let surfaces = vec![
            ZSurface::new(0, "surface1", Rect::new(0, 0, 100, 100)),
            ZSurface::new(0, "surface2", Rect::new(100, 0, 200, 200)),
            ZSurface::new(0, "surface2", Rect::new(0, 100, 100, 200)),
        ];

        for s in surfaces.iter().cloned() {
            assert!(tree.insert(s));
        }

        let expected = surfaces;

        assert_eq!(expected, tree.flatten());
    }

    #[test]
    fn hidden_surface_not_kept() {
        let mut tree = ZTree::new();
        let lower = ZSurface::new(0, "lower", Rect::new(10, 10, 20, 20));
        let upper = ZSurface::new(1, "upper", Rect::new(0, 0, 100, 100));

        assert!(tree.insert(lower));
        assert!(tree.insert(upper.clone()));

        let expected = vec![upper];

        assert_eq!(expected, tree.flatten());
    }

    #[test]
    fn hidden_surface_no_insert() {
        let mut tree = ZTree::new();
        let lower = ZSurface::new(0, "lower", Rect::new(10, 10, 20, 20));
        let upper = ZSurface::new(1, "upper", Rect::new(0, 0, 100, 100));

        assert!(tree.insert(upper.clone()));
        assert!(!tree.insert(lower));

        let expected = vec![upper];

        assert_eq!(expected, tree.flatten());
    }

    #[test]
    fn multi_layers_no_overlap() {
        let mut tree = ZTree::new();

        let mut surfaces = vec![
            ZSurface::new(3, "surface1", Rect::new(0, 0, 100, 100)),
            ZSurface::new(1, "surface2", Rect::new(100, 0, 200, 200)),
            ZSurface::new(2, "surface2", Rect::new(0, 100, 100, 200)),
        ];

        for s in surfaces.iter().cloned() {
            assert!(tree.insert(s));
        }

        surfaces.sort_by_key(|s| s.z_index);
        let expected = surfaces;

        assert_eq!(expected, tree.flatten())
    }

    #[test]
    fn multi_layers_partial_overlap() {
        let mut tree = ZTree::new();

        let mut surfaces = vec![
            ZSurface::new(3, "surface1", Rect::new(0, 0, 100, 100)),
            ZSurface::new(1, "surface2", Rect::new(50, 0, 200, 200)),
            ZSurface::new(2, "surface3", Rect::new(0, 100, 150, 200)),
        ];

        for s in surfaces.iter().cloned() {
            tree.insert(s);
        }

        let lowest_surface = ZSurface::new(1, "surface2", Rect::new(100, 0, 200, 200));

        surfaces.sort_by_key(|s| s.z_index);
        let expected: Vec<_> = std::iter::once(lowest_surface).chain(surfaces.into_iter().skip(1)).collect();

        assert_eq!(expected, tree.flatten())
    }

    #[test]
    fn smallest_bounding_box_returned() {
        let mut tree = ZTree::new();

        let surfaces = vec![
            ZSurface::new(1, "surface1", Rect::new(0, 0, 200, 200)),
            ZSurface::new(2, "surface2", Rect::new(50, 0, 150, 100)),
            ZSurface::new(3, "surface3", Rect::new(0, 100, 200, 200))
        ];

        for s in surfaces.iter().cloned() {
            tree.insert(s);
        }

        let expected_lowest = ZSurface::new(1, "surface1", Rect::new(0, 0, 200, 100));
        let expected: Vec<_> = std::iter::once(expected_lowest).chain(surfaces.into_iter().skip(1)).collect();

        assert_eq!(expected, tree.flatten())
    }
}
