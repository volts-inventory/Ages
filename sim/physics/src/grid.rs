//! Hex grid covering the planet.
//!
//! M1a foundation uses a flat 2D hex lattice with periodic
//! boundaries (torus topology). Every cell has exactly six
//! neighbours. Simple, deterministic, and lets the rest of M1a's
//! physics get built and validated against a uniform topology.
//!
//! TODO(M1b or later): replace with a spherical Goldberg polyhedron
//! (geodesic icosahedron, dual). Goldberg has 12 pentagon cells (5
//! neighbours instead of 6) at the icosahedron vertices. The `Grid`
//! API here is shaped so the swap is local — `neighbours` returns a
//! variable-length list, callers don't assume 6.

use core::cmp::Ordering;

/// Axial hex coordinates `(q, r)`. Standard hex-grid convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Axial {
    pub q: i32,
    pub r: i32,
}

impl Axial {
    pub const fn new(q: i32, r: i32) -> Self {
        Self { q, r }
    }
}

impl Ord for Axial {
    fn cmp(&self, other: &Self) -> Ordering {
        // Sort by (r, q) for deterministic iteration. r-major is
        // arbitrary but fixed; the iteration order is what matters.
        self.r.cmp(&other.r).then(self.q.cmp(&other.q))
    }
}

impl PartialOrd for Axial {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Cell index into the grid's flat storage. Stable across a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellId(pub u32);

/// A flat hex grid over a `width × height` parallelogram with
/// periodic boundaries. `n_cells = width * height`.
#[derive(Debug, Clone)]
pub struct HexGrid {
    width: u32,
    height: u32,
    /// Cells indexed in iteration order — sorted by `(r, q)` axial
    /// coordinates so deterministic iteration is just a slice walk.
    cells: Vec<Axial>,
}

impl HexGrid {
    /// Build a `width × height` torus hex grid.
    pub fn new(width: u32, height: u32) -> Self {
        let mut cells = Vec::with_capacity((width * height) as usize);
        for r in 0..i32::try_from(height).expect("grid height fits in i32") {
            for q in 0..i32::try_from(width).expect("grid width fits in i32") {
                cells.push(Axial { q, r });
            }
        }
        // Already in (r, q) order by construction; assert in debug.
        debug_assert!(cells.windows(2).all(|w| w[0] < w[1]));
        Self {
            width,
            height,
            cells,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn n_cells(&self) -> usize {
        self.cells.len()
    }

    /// Iterate cells in canonical order — sorted by (r, q). This is
    /// the order events get emitted in.
    pub fn cells(&self) -> impl Iterator<Item = (CellId, Axial)> + '_ {
        self.cells
            .iter()
            .copied()
            .enumerate()
            .map(|(i, a)| (CellId(u32::try_from(i).expect("cell id fits in u32")), a))
    }

    /// Look up the axial coordinates of a cell by its id. Cell
    /// storage is row-major `(r, q)` with width `self.width`, so
    /// the formula is direct: `q = id % width`, `r = id / width`.
    /// Used by territory BFS where we need to walk neighbours from
    /// a cell id without iterating the full cell list.
    pub fn axial_of(&self, cell: CellId) -> Axial {
        let id = cell.0;
        let q = i32::try_from(id % self.width).expect("q fits in i32");
        let r = i32::try_from(id / self.width).expect("r fits in i32");
        Axial { q, r }
    }

    /// Look up a cell by axial coordinates, applying torus
    /// wraparound. None if the indices are negative beyond useful
    /// range; in practice neighbour lookup wraps via mod.
    pub fn cell_id(&self, axial: Axial) -> CellId {
        let width = i32::try_from(self.width).expect("width fits in i32");
        let height = i32::try_from(self.height).expect("height fits in i32");
        let wrapped_q = axial.q.rem_euclid(width);
        let wrapped_r = axial.r.rem_euclid(height);
        CellId(u32::try_from(wrapped_r * width + wrapped_q).expect("cell id fits in u32"))
    }

    /// Six neighbours of a cell, in canonical order (E, NE, NW, W,
    /// SW, SE). Order is fixed for determinism. Torus wraparound
    /// applied automatically.
    pub fn neighbours(&self, a: Axial) -> [CellId; 6] {
        // Axial neighbour offsets, fixed canonical order.
        const OFFSETS: [(i32, i32); 6] = [
            (1, 0),  // E
            (1, -1), // NE
            (0, -1), // NW
            (-1, 0), // W
            (-1, 1), // SW
            (0, 1),  // SE
        ];
        let mut out = [CellId(0); 6];
        for (i, (dq, dr)) in OFFSETS.iter().enumerate() {
            out[i] = self.cell_id(Axial {
                q: a.q + dq,
                r: a.r + dr,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iteration_is_deterministic() {
        let g = HexGrid::new(4, 3);
        let a: Vec<_> = g.cells().collect();
        let b: Vec<_> = g.cells().collect();
        assert_eq!(a, b);
    }

    #[test]
    fn n_cells_matches_dimensions() {
        let g = HexGrid::new(4, 3);
        assert_eq!(g.n_cells(), 12);
    }

    #[test]
    fn neighbours_wrap_at_torus_boundary() {
        let g = HexGrid::new(3, 3);
        let corner = Axial::new(0, 0);
        let n = g.neighbours(corner);
        // None of the six neighbours should fall outside the grid;
        // the torus must wrap.
        let n_cells = u32::try_from(g.n_cells()).expect("n_cells fits in u32");
        for cid in n {
            assert!(cid.0 < n_cells);
        }
    }

    #[test]
    fn neighbour_order_is_canonical() {
        let g = HexGrid::new(5, 5);
        let centre = Axial::new(2, 2);
        let n_a = g.neighbours(centre);
        let n_b = g.neighbours(centre);
        assert_eq!(n_a, n_b);
    }

    #[test]
    fn axial_ordering_is_r_major() {
        let a = Axial::new(5, 0);
        let b = Axial::new(0, 1);
        assert!(a < b, "(5,0) should sort before (0,1) under r-major");
    }
}
