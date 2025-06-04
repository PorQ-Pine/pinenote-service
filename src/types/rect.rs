use std::fmt::Debug;

#[derive(Debug, PartialEq, Clone)]
pub struct Rect {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32
}

impl Rect {
    pub const fn new(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    pub fn intersect(&self, rhs: &Self) -> bool {
        self.x1 <= rhs.x2 &&
            self.x2 >= rhs.x1 &&
            self.y1 <= rhs.y2 &&
            self.y2 >= rhs.y1
    }

    pub fn cover(&self, other: &Self) -> bool {
        self.x1 <= other.x1 &&
            self.y1 <= other.y1 &&
            self.x2 >= other.x2 &&
            self.y2 >= other.y2
    }

    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let inter = Rect::new(
            i32::max(self.x1, other.x1),
            i32::max(self.y1, other.y1),
            i32::min(self.x2, other.x2),
            i32::min(self.y2, other.y2)
        );

        if (inter.x2 - inter.x1) <= 0 || (inter.y2 - inter.y1) <= 0 {
            None
        } else {
            Some(inter)
        }
    }
}

/// Rectagle, possibly split to mask part of it.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitRect(Vec<Rect>);

impl SplitRect {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return the split rectangle bounding box
    pub fn bounds(&self) -> Option<Rect> {
        if self.is_empty() { None } else {
            Some(self.0.iter().fold(
                Rect::new(i32::MAX, i32::MAX, i32::MIN, i32::MIN),
                |racc, r| Rect::new(
                    i32::min(racc.x1, r.x1),
                    i32::min(racc.y1, r.y1),
                    i32::max(racc.x2, r.x2),
                    i32::max(racc.y2, r.y2)
                )
            ))
        }
    }

    /// Mask a rectangle with another, and return a split rectangle containing
    /// the unmasked parts.
    fn mask_rect(r: Rect, other: &Rect) -> Self {
        let Some(inter) = r.intersection(other) else { return Self(vec![r]) };

        [
            Rect::new(r.x1, r.y1, inter.x1, inter.y2),
            Rect::new(inter.x1, r.y1, r.x2, inter.y1),
            Rect::new(inter.x2, inter.y1, r.x2, r.y2),
            Rect::new(r.x1, inter.y2, inter.x2, r.y2),
        ].into_iter()
            .filter(|r| (r.x2 - r.x1) > 0 && (r.y2 - r.y1) > 0)
            .collect()
    }

    /// Create a new split rectangle by masking part of it.
    pub fn mask_with(self, other: &Rect) -> SplitRect {
        Self(self.0.into_iter()
            .flat_map(|r| Self::mask_rect(r, other))
            .collect())
    }
}

impl FromIterator<Rect> for SplitRect {
    fn from_iter<T: IntoIterator<Item = Rect>>(iter: T) -> Self {
        Self(Vec::from_iter(iter))
    }
}

impl IntoIterator for SplitRect {
    type Item = Rect;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl From<Rect> for SplitRect {
    fn from(value: Rect) -> Self {
        Self(vec![value])
    }
}

#[cfg(test)]
pub mod tests {
    use super::{Rect, SplitRect};

    #[test]
    fn no_inter() {
        let sr = SplitRect::from(Rect::new(10, 10, 20, 20));
        let r2 = Rect::new(30, 30, 40, 40);

        let expected = sr.clone();
        let res = sr.mask_with(&r2);

        assert_eq!(Some(Rect::new(10, 10, 20, 20)), res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn full_cover() {
        let sr = SplitRect::from(Rect::new(100, 100, 150, 150));
        let r2 = Rect::new(50, 50, 200, 200);

        let res = sr.mask_with(&r2);

        assert!(res.is_empty());
        assert_eq!(None, res.bounds());
    }

    #[test]
    fn same_size() {
        let r = Rect::new(100, 100, 150, 150);
        let sr = SplitRect::from(r.clone());

        let res = sr.mask_with(&r);

        assert!(res.is_empty());
        assert_eq!(None, res.bounds());
    }

    #[test]
    fn same_size_multi() {
        let bound = Rect::new(100, 100, 200, 200);
        let sr = SplitRect(vec![
            Rect::new(100, 100, 200, 120),
            Rect::new(150, 120, 200, 200),
            Rect::new(100, 120, 150, 200),
        ]);

        let res = sr.mask_with(&bound);

        assert!(res.is_empty());
        assert_eq!(None, res.bounds());
    }

    #[test]
    fn cover_center() {
        let rect = Rect::new(100, 100, 200, 200);
        let sr = SplitRect::from(rect.clone());
        let r2 = Rect::new(120, 130, 140, 150);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(rect);
        let expected = SplitRect(vec![
            Rect::new(100, 100, 120, 150),
            Rect::new(120, 100, 200, 130),
            Rect::new(140, 130, 200, 200),
            Rect::new(100, 150, 140, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_center_horiz() {
        let rect = Rect::new(100, 100, 200, 200);
        let sr = SplitRect::from(rect.clone());
        let r2 = Rect::new(20, 130, 240, 150);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(rect);
        let expected = SplitRect(vec![
            Rect::new(100, 100, 200, 130),
            Rect::new(100, 150, 200, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_center_vert() {
        let rect = Rect::new(100, 100, 200, 200);
        let sr = SplitRect::from(rect.clone());
        let r2 = Rect::new(120, 30, 140, 250);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(rect);
        let expected = SplitRect(vec![
            Rect::new(100, 100, 120, 200),
            Rect::new(140, 100, 200, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_top() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(50, 50, 250, 150);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 150, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 150, 200, 200),
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_left() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(50, 50, 150, 250);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(150, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(150, 100, 200, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_right() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(150, 50, 250, 250);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 150, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 150, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_bottom() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(50, 150, 250, 250);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 150));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 200, 150),
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_top_left() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(50, 50, 130, 150);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(130, 100, 200, 200),
            Rect::new(100, 150, 130, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_top_right() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(130, 50, 230, 150);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 130, 150),
            Rect::new(100, 150, 200, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_bottom_left() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(50, 120, 130, 250);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 200, 120),
            Rect::new(130, 120, 200, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_bottom_right() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(110, 120, 230, 250);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 110, 200),
            Rect::new(110, 100, 200, 120)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_top_part() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(110, 50, 150, 130);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 110, 130),
            Rect::new(150, 100, 200, 200),
            Rect::new(100, 130, 150, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_left_part() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(50, 110, 130, 150);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 200, 110),
            Rect::new(130, 110, 200, 200),
            Rect::new(100, 150, 130, 200)

        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_right_part() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(130, 110, 250, 150);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 130, 150),
            Rect::new(130, 100, 200, 110),
            Rect::new(100, 150, 200, 200)

        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }

    #[test]
    fn cover_bottom_part() {
        let sr = SplitRect::from(Rect::new(100, 100, 200, 200));
        let r2 = Rect::new(110, 130, 150, 250);

        let res = sr.mask_with(&r2);

        let expected_bounds = Some(Rect::new(100, 100, 200, 200));
        let expected = SplitRect(vec![
            Rect::new(100, 100, 110, 200),
            Rect::new(110, 100, 200, 130),
            Rect::new(150, 130, 200, 200)
        ]);

        assert_eq!(expected_bounds, res.bounds());
        assert_eq!(expected, res);
    }
}

