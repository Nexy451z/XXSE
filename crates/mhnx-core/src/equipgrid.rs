//! The equipment box as the game draws it: a fixed grid of slots, not a list.
//!
//! The box holds 2000 entries and the game shows them ten to a row, ten rows to
//! a page, twenty pages. A slot that is empty is drawn as an empty slot - it is
//! part of the grid, which is why the cursor can sit on one and why "new" can
//! put a piece there.
//!
//! This module is only the arithmetic: which slot a page and a cell name, where
//! the cursor is, and what one press moves it to. It exists so the console and
//! the host build share one answer and the tests can pin the edges.

use crate::equip::EQUIP_COUNT;

/// How many slots across one page.
pub const COLUMNS: usize = 10;
/// How many rows down one page.
pub const ROWS: usize = 10;
/// Slots on one page.
pub const PER_PAGE: usize = COLUMNS * ROWS;
/// Every slot in the box.
pub const SLOTS: usize = EQUIP_COUNT;
/// Pages the box is shown in.
pub const PAGES: usize = SLOTS / PER_PAGE;

/// The slot a page's row and column name.
///
/// Out of range arguments clamp into the page rather than wrapping into a
/// neighbouring one: a cursor that ran off the right edge of page 3 has to land
/// on page 3's left edge, not on page 4's.
pub fn slot_at(page: usize, row: usize, column: usize) -> usize {
    let page = page.min(PAGES.saturating_sub(1));
    let row = row.min(ROWS - 1);
    let column = column.min(COLUMNS - 1);
    (page * PER_PAGE + row * COLUMNS + column).min(SLOTS - 1)
}

/// Which page a slot is on.
pub fn page_of(slot: usize) -> usize {
    (slot.min(SLOTS - 1)) / PER_PAGE
}

/// Which row of its page a slot is in.
pub fn row_of(slot: usize) -> usize {
    (slot.min(SLOTS - 1)) % PER_PAGE / COLUMNS
}

/// Which column of its page a slot is in.
pub fn column_of(slot: usize) -> usize {
    (slot.min(SLOTS - 1)) % COLUMNS
}

/// The first slot of a page.
pub fn first_slot_of(page: usize) -> usize {
    page.min(PAGES.saturating_sub(1)) * PER_PAGE
}

/// Where the cursor is in the box, and where one press takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    slot: usize,
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

impl Cursor {
    /// On the first slot of the first page.
    pub fn new() -> Self {
        Cursor { slot: 0 }
    }

    /// On a particular slot, clamped into the box.
    pub fn at(slot: usize) -> Self {
        Cursor {
            slot: slot.min(SLOTS - 1),
        }
    }

    /// The slot the cursor is on.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// The page the cursor is on.
    pub fn page(&self) -> usize {
        page_of(self.slot)
    }

    /// The row the cursor is on, within its page.
    pub fn row(&self) -> usize {
        row_of(self.slot)
    }

    /// The column the cursor is on, within its page.
    pub fn column(&self) -> usize {
        column_of(self.slot)
    }

    /// Move to a slot the caller worked out itself.
    pub fn set(&mut self, slot: usize) {
        self.slot = slot.min(SLOTS - 1);
    }

    /// One press of a direction.
    ///
    /// Left and right stop at the page's edge instead of wrapping onto the
    /// neighbouring row: the game does the same, and a horizontal press that
    /// silently changed rows would be the worst kind of surprise in a grid.
    /// Up and down do the same at the top and bottom of a page.
    pub fn step(&mut self, dx: isize, dy: isize) {
        let row = self.row() as isize + dy;
        let column = self.column() as isize + dx;
        let row = row.clamp(0, ROWS as isize - 1) as usize;
        let column = column.clamp(0, COLUMNS as isize - 1) as usize;
        self.slot = slot_at(self.page(), row, column);
    }

    /// One press of a direction, turning the page at the edge.
    ///
    /// This is what the box screen uses: with 2000 slots and no free key for a
    /// page turn, reading order is the answer - right from the last column of a
    /// page lands on the same row of the next page, down from the last row
    /// lands on the first row of the next page, and the same backwards.
    ///
    /// [`Cursor::step`] is the clamping version, used where a page turn must not
    /// be a side effect of a direction press.
    pub fn step_paged(&mut self, dx: isize, dy: isize) {
        let row = self.row() as isize + dy;
        let column = self.column() as isize + dx;
        if (0..ROWS as isize).contains(&row) && (0..COLUMNS as isize).contains(&column) {
            self.slot = slot_at(self.page(), row as usize, column as usize);
            return;
        }
        // Past an edge: work out how many pages that is worth and land on the
        // matching cell of the page we arrive at.
        let mut page = self.page() as isize;
        let mut row = row;
        let mut column = column;
        if column >= COLUMNS as isize {
            page += 1;
            column = 0;
        } else if column < 0 {
            page -= 1;
            column = COLUMNS as isize - 1;
        }
        if row >= ROWS as isize {
            page += 1;
            row = 0;
        } else if row < 0 {
            page -= 1;
            row = ROWS as isize - 1;
        }
        // Past the first or the last page there is nowhere to go: stay put
        // rather than snapping to a different cell of the page we are on.
        if page < 0 || page >= PAGES as isize {
            return;
        }
        self.slot = slot_at(page as usize, row as usize, column as usize);
    }

    /// Turn the page, keeping the cursor in the same cell of the new page.
    pub fn turn_page(&mut self, pages: isize) {
        let page = (self.page() as isize + pages).clamp(0, PAGES as isize - 1) as usize;
        self.slot = slot_at(page, self.row(), self.column());
    }

    /// Jump to the first cell of a page.
    pub fn go_to_page(&mut self, page: usize) {
        self.slot = slot_at(page, 0, 0);
    }

    /// Is the cursor on the first slot of its page?
    pub fn at_page_start(&self) -> bool {
        self.row() == 0 && self.column() == 0
    }

    /// Is the cursor on the last slot of its page?
    pub fn at_page_end(&self) -> bool {
        self.row() == ROWS - 1 && self.column() == COLUMNS - 1
    }
}
