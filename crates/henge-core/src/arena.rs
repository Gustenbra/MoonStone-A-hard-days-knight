//! A combat arena: a backdrop, some scenery, and the rectangle the fighters may
//! stand in. Moonstone's arenas are a flat walkable band with scenery drawn over
//! it, so depth is decided purely by feet position.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

impl Bounds {
    /// Tolerates inverted bounds rather than panicking, since content is data and
    /// data can be wrong.
    pub fn clamp(&self, x: i32, y: i32) -> (i32, i32) {
        let (l, r) = (self.left.min(self.right), self.left.max(self.right));
        let (t, b) = (self.top.min(self.bottom), self.top.max(self.bottom));
        (x.clamp(l, r), y.clamp(t, b))
    }

    pub fn is_sane(&self) -> bool {
        self.left < self.right
            && self.top < self.bottom
            && (0..640).contains(&self.right)
            && (0..400).contains(&self.bottom)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
}

/// One piece of scenery stamped onto the arena.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prop {
    pub sheet: u8,
    pub cell: u8,
    pub x: i16,
    pub y: i16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Arena {
    pub name: String,
    pub backdrop: String,
    pub sheets: Vec<String>,
    pub bounds: Bounds,
    pub props: Vec<Prop>,
}

impl Arena {
    /// Scenery and actors are drawn together, sorted by feet, so a fighter walking
    /// behind a rock is occluded by it and one walking in front covers it.
    pub fn draw_order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.props.len()).collect();
        idx.sort_by_key(|&i| self.props[i].y);
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamping_keeps_actors_inside_the_walkable_band() {
        let b = Bounds { left: 0, right: 319, top: 10, bottom: 114 };
        assert_eq!(b.clamp(-40, 200), (0, 114));
        assert_eq!(b.clamp(400, 0), (319, 10));
        assert!(b.contains(160, 60));
        assert!(!b.contains(160, 150));
    }

    #[test]
    fn inverted_bounds_do_not_panic() {
        let b = Bounds { left: 35584, right: 0, top: 0, bottom: 0 };
        assert_eq!(b.clamp(100, 100), (100, 0));
        assert!(!b.is_sane());
    }

    #[test]
    fn props_sort_back_to_front() {
        let a = Arena {
            name: "t".into(), backdrop: "b".into(), sheets: vec![],
            bounds: Bounds { left: 0, right: 319, top: 10, bottom: 114 },
            props: vec![
                Prop { sheet: 0, cell: 0, x: 0, y: 90 },
                Prop { sheet: 0, cell: 1, x: 0, y: 30 },
            ],
        };
        assert_eq!(a.draw_order(), vec![1, 0]);
    }
}
