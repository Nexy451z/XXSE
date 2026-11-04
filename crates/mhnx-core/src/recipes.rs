//! The smithy's recipes: what a weapon, an armour piece or a decoration costs,
//! and the reverse index from a material to everything that asks for it.
//!
//! The editor shows the game's own tables; this module answers the question the
//! tables do not answer on their own - "what do I need to make this", and "what
//! can I make out of this". Like [crate::database] it ships no game data: the
//! caller hands over the bytes of files from its own dump and gets recipes
//! back. Nothing here reads a filesystem, nothing here writes anything, and a
//! dump that is not there is an empty [Book] rather than an error.
//!
//! ## The five tables
//!
//! Measured on a retail Japanese MHXX dump, `nativeNX/table`. Every one of the
//! 35 files starts with the same 8 byte header - an `f32` version 1.0, then a
//! `u32` count - and every one satisfies `8 + count * record == file size`
//! exactly, which is what [Book::add] checks before it reads a byte:
//!
//! ```text
//! file                        record  files  records  what it is
//! weaponCreateW00..W14.wcd        38     14    1,152   forging a weapon
//! weaponProcessW00..W14.wpd       32     14   10,911   levelling a weapon up
//! armorCreateA00..A04.acrd        42      5    2,720   forging one armour part
//! armorProcessA00.apd             38      1    5,162   levelling an armour series
//! decoCreate.dcd                  16      1      275   forging a decoration
//! ```
//!
//! Those 20,220 rows expand to **28,623 recipes**, because a row can name more
//! than one product: `armorCreate` carries up to four armour series in one
//! record (the male and female forms of one design share a recipe), and
//! `armorProcess` the same four. The expansion is 1,152 + 10,911 + 5,787 +
//! 10,498 + 275, and it matches the reference tool's `recipes.csv` row for
//! row: 28,623 there, 28,623 here, and every weapon class and armour part the
//! same (class 0 gives 1,053 = 95 forge plus 958 process rows on both sides).
//! Two independent readers of the same tables agreeing on every count is what
//! makes the offsets below trustworthy.
//!
//! ## The records
//!
//! Offsets are byte offsets inside one record; a field this module does not
//! read is named `unread` rather than given a meaning it was not measured to
//! have. Material entries are either `(u16 item, u8 count, u8 flag)` -
//! "long", 4 bytes, in the two create tables - or `(u16 item, u8 count)` -
//! "short", 3 bytes, in the two process tables and the decoration table. An
//! entry whose item is 0 is unused and is returned as [Material::NONE].
//!
//! ```text
//! weaponCreate   +0x00 u16 group (1..8, unread)   +0x02 u16 weapon id
//!                +0x06 4 x long material          +0x16 u16 provision group
//!                +0x18 u8 provision value         +0x19 unread
//!                +0x1A 3 x long scrap             (ends at 0x26)
//! weaponProcess  +0x00 u16 weapon id              +0x02 u16 level (1..15)
//!                +0x04 4 x short material         +0x10 provision
//!                +0x13 unread                     +0x14 3 x long scrap
//! armorCreate    +0x00 4 x u16 armour series      +0x08 u16 unread
//!                +0x0A 4 x long material          +0x1A provision
//!                +0x1D unread                     +0x1E 3 x long scrap
//! armorProcess   +0x00 4 x u16 armour series      +0x08 u16 level (2..15)
//!                +0x0A 4 x short material         +0x16 provision
//!                +0x19 unread                     +0x1A 3 x long scrap
//! decoCreate     +0x00 u16 decoration item        +0x02 u16 unread (0..2)
//!                +0x04 4 x short material
//! ```
//!
//! The provision is a second way to pay: a group number and a value. The
//! game's own help text (`LbShop_EqMakeMsg`) asks for materials of one group
//! adding up to the value, so the two fields are kept as measured; this module
//! does not reinterpret them and the interface shows them as the numbers they
//! are.
//!
//! The 12 byte tail of the four smithy tables is the material a recipe **gives
//! back**: three `(u16 item, u8 count, u8 pad)` entries, and every item id that
//! turns up there - 344 of them - is a 端材 (scrap) item, 2158 `鉱石の端材`
//! through 2501 `異形の真端材`, with a count of 1 or 2. That is what makes the
//! reading of the tail certain rather than plausible.
//!
//! ## What is *not* read
//!
//! * `weaponCreate` +0x00 (values 1..8) and `armorCreate` +0x08 (0, 1, 4..~200):
//!   neither is a product id and neither changes what a recipe costs, so they
//!   stay unread rather than being given a name.
//! * `decoCreate` +0x02 (0, 1 or 2): it looks like a count of something and is
//!   not needed to read the materials.
//! * Every file that failed the header check is counted in [Book::skipped]
//!   instead of being half read.
//!
//! ## Two recipes that look like a misread and are not
//!
//! The material ids of some rows are small enough that the first reading of
//! them was "that cannot be an item". They are:
//!
//! * 25 rows of the five `armorCreate` tables - the `マカルパ` set, five parts
//!   and five ranks - whose fourth material is `調合書①` (1) for the base set,
//!   `調合書②` (2) for `マカルパS`, `③` for `U`, `④` for `X` and `⑤` for `Z`,
//!   next to fish (`春夜鯉`, `ハリマグロ`, `カジキマグロ`, `金剛魚`). A set built
//!   out of books and fish is exactly what this row says it is.
//! * five rows of `decoCreate`: `攻撃珠【２】`, `攻撃珠【３】`, `強走珠【１】`,
//!   `観察珠【１】` and `博士珠【１】`, which ask for `鬼人薬` (17),
//!   `鬼人薬グレート` (18), `強走薬グレート` (16), `双眼鏡` (8) and
//!   `調合書①` (1). Decorations really are made from 鬼人薬.
//!
//! Both are kept as stored, and both are in the reference tool's CSV the same
//! way, so no row is filtered out by a guess about what a plausible item id
//! looks like.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Header of every recipe table, before the first record.
pub const HEADER: usize = 8;
/// Version word of every recipe table: `1.0f32` little endian.
pub const VERSION: [u8; 4] = [0x00, 0x00, 0x80, 0x3f];

/// Material slots in a recipe.
pub const MATERIALS: usize = 4;
/// Scrap slots in a recipe's tail (0 for the decoration table).
pub const RETURNED: usize = 3;

/// Bytes per `weaponCreateWNN` record.
pub const WEAPON_CREATE_RECORD: usize = 38;
/// Bytes per `weaponProcessWNN` record.
pub const WEAPON_PROCESS_RECORD: usize = 32;
/// Bytes per `armorCreateANN` record.
pub const ARMOR_CREATE_RECORD: usize = 42;
/// Bytes per `armorProcessA00` record.
pub const ARMOR_PROCESS_RECORD: usize = 38;
/// Bytes per `decoCreate` record.
pub const DECO_CREATE_RECORD: usize = 16;

/// Armour parts the game names, in the order the five create tables use.
pub const ARMOR_PARTS: u8 = 5;

/// Level a row of a create table carries: forging, not upgrading.
pub const FORGE: u16 = 0;

/// One material of a recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Material {
    /// Item id, 0 when the slot is unused.
    pub item: u16,
    /// How many the recipe asks for (or gives back).
    pub count: u8,
}

impl Material {
    /// An unused slot.
    pub const NONE: Material = Material { item: 0, count: 0 };

    /// Whether this slot names an item.
    pub fn is_used(self) -> bool {
        self.item != 0
    }
}

/// How else a recipe can be paid for: a group of items and the value they have
/// to add up to. A zero group means "not used".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Provision {
    /// Group number the game asks for.
    pub group: u16,
    /// Value that group has to reach.
    pub value: u8,
}

impl Provision {
    /// A recipe that asks for nothing of the kind.
    pub const NONE: Provision = Provision { group: 0, value: 0 };

    /// Whether this recipe has a provision at all.
    pub fn is_used(self) -> bool {
        self.group != 0
    }
}

/// What a recipe makes.
///
/// The variant is the kind of product, and the field that follows is the id the
/// game's own table uses for it: a weapon id inside one weapon class, an armour
/// *series* (the save's armour id; the part picks one of its five pieces), or a
/// decoration's item id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Product {
    /// A weapon, by class (the JP class number, 0..14 without 5) and id.
    Weapon {
        /// Weapon class, the NN of `weaponCreateWNN`.
        class: u8,
        /// Weapon id inside that class.
        id: u16,
    },
    /// One piece of an armour series, forged at the smithy.
    Armor {
        /// Armour series, the id a save stores.
        series: u16,
        /// Which piece, 1 (head) through 5 (legs).
        part: u8,
    },
    /// An armour series levelled up: the game stores one recipe per series and
    /// level, and it applies to every piece of that series.
    ArmorSeries {
        /// Armour series.
        series: u16,
    },
    /// A decoration, by the item id the smithy produces.
    Deco {
        /// Decoration item id.
        item: u16,
    },
}

impl Product {
    /// The equipment type a save records for this product, or 0 for a
    /// decoration (which is an item, not equipment).
    ///
    /// The JP weapon classes sit five below the save's types and have their own
    /// gap at 5, so `class + 7` maps one to the other exactly - the same
    /// mapping [crate::equip] documents.
    pub fn equip_kind(self) -> u8 {
        match self {
            Product::Weapon { class, .. } => class.saturating_add(7),
            Product::Armor { part, .. } => part,
            Product::ArmorSeries { .. } => 1,
            Product::Deco { .. } => 0,
        }
    }

    /// The id the game's equipment table uses for this product, or the item id
    /// for a decoration.
    pub fn equip_id(self) -> u16 {
        match self {
            Product::Weapon { id, .. } => id,
            Product::Armor { series, .. } | Product::ArmorSeries { series } => series,
            Product::Deco { item } => item,
        }
    }

    /// Whether this is a weapon or a piece of armour, i.e. something the 装備
    /// tab can show.
    pub fn is_equipment(self) -> bool {
        !matches!(self, Product::Deco { .. })
    }
}

/// One recipe: a product, the level it reaches, and what it costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe {
    /// What it makes.
    pub product: Product,
    /// The level the product has afterwards: [FORGE] for forging, 1 or more for
    /// an upgrade.
    pub level: u16,
    /// Up to [MATERIALS] materials; unused slots are [Material::NONE].
    pub materials: [Material; MATERIALS],
    /// The second way to pay, if the recipe has one.
    pub provision: Provision,
    /// Up to [RETURNED] scrap items the recipe gives back.
    pub returned: [Material; RETURNED],
}

impl Recipe {
    /// Whether this row forges the product rather than levelling it.
    pub fn is_forge(&self) -> bool {
        self.level == FORGE
    }

    /// The materials that are actually used, in slot order.
    pub fn material_list(&self) -> Vec<Material> {
        self.materials
            .iter()
            .copied()
            .filter(|m| m.is_used())
            .collect()
    }

    /// The scrap the recipe gives back, in slot order.
    pub fn returned_list(&self) -> Vec<Material> {
        self.returned
            .iter()
            .copied()
            .filter(|m| m.is_used())
            .collect()
    }
}

/// Which table a file name names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    /// `weaponCreateWNN.wcd`, one weapon class.
    WeaponCreate(u8),
    /// `weaponProcessWNN.wpd`, one weapon class.
    WeaponProcess(u8),
    /// `armorCreateANN.acrd`, one armour part (1..=5).
    ArmorCreate(u8),
    /// `armorProcessA00.apd`, every armour series.
    ArmorProcess,
    /// `decoCreate.dcd`.
    DecoCreate,
}

impl TableKind {
    /// The record size of the table, in bytes.
    pub fn record_size(self) -> usize {
        match self {
            TableKind::WeaponCreate(_) => WEAPON_CREATE_RECORD,
            TableKind::WeaponProcess(_) => WEAPON_PROCESS_RECORD,
            TableKind::ArmorCreate(_) => ARMOR_CREATE_RECORD,
            TableKind::ArmorProcess => ARMOR_PROCESS_RECORD,
            TableKind::DecoCreate => DECO_CREATE_RECORD,
        }
    }
}

/// Which table a file name names, without reading the file.
///
/// Case does not matter. A bare file name is what this expects: the folder, if
/// a caller left one on, is ignored rather than matched, so `table/x.wcd` and
/// `x.wcd` answer the same. `None` means "not one of the five", which is the
/// normal answer for the 214 other files of `nativeNX/table`.
pub fn kind_of(file: &str) -> Option<TableKind> {
    let lower = file.to_ascii_lowercase();
    let (stem, ext) = lower.rsplit_once('.')?;
    // A caller may hand over a path; only the last component names the table.
    let stem = stem.rsplit('/').next().unwrap_or(stem);
    if ext == "wcd" {
        return number_after(stem, "weaponcreatew").map(TableKind::WeaponCreate);
    }
    if ext == "wpd" {
        return number_after(stem, "weaponprocessw").map(TableKind::WeaponProcess);
    }
    if ext == "acrd" {
        // The file names the part zero based, the recipe names it one based.
        let index = number_after(stem, "armorcreatea")?;
        if index >= ARMOR_PARTS {
            return None;
        }
        return Some(TableKind::ArmorCreate(index + 1));
    }
    if ext == "apd" && stem.starts_with("armorprocessa") {
        return Some(TableKind::ArmorProcess);
    }
    if ext == "dcd" && stem.starts_with("decocreate") {
        return Some(TableKind::DecoCreate);
    }
    None
}

/// The two digit number a stem ends with, after a prefix.
fn number_after(stem: &str, prefix: &str) -> Option<u8> {
    let rest = stem.strip_prefix(prefix)?;
    if rest.len() != 2 || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u8>().ok()
}

/// The records of a table, or `None` when the bytes are not one.
///
/// The header and the exact length are checked together: a file whose count
/// does not fill it to the byte is refused rather than read up to the point
/// where the records run out, because a partly read recipe table would show
/// recipes that are not in the game.
fn records(bytes: &[u8], record: usize) -> Option<&[u8]> {
    if record == 0 || bytes.len() < HEADER {
        return None;
    }
    if bytes.get(..4) != Some(VERSION.as_slice()) {
        return None;
    }
    let count = u32::from_le_bytes([
        *bytes.get(4)?,
        *bytes.get(5)?,
        *bytes.get(6)?,
        *bytes.get(7)?,
    ]) as usize;
    let end = HEADER.checked_add(count.checked_mul(record)?)?;
    if end != bytes.len() {
        return None;
    }
    bytes.get(HEADER..end)
}

/// Little endian `u16`, or 0 when the record is too short to hold it.
fn u16_at(record: &[u8], at: usize) -> u16 {
    match at.checked_add(2).and_then(|end| record.get(at..end)) {
        Some(bytes) => u16::from_le_bytes([bytes[0], bytes[1]]),
        None => 0,
    }
}

/// One byte, or 0 when the record is too short.
fn u8_at(record: &[u8], at: usize) -> u8 {
    record.get(at).copied().unwrap_or(0)
}

/// [MATERIALS] material slots, long (4 byte) or short (3 byte) ones.
fn materials(record: &[u8], at: usize, stride: usize) -> [Material; MATERIALS] {
    let mut out = [Material::NONE; MATERIALS];
    for (slot, entry) in out.iter_mut().enumerate() {
        let start = at + stride * slot;
        *entry = Material {
            item: u16_at(record, start),
            count: u8_at(record, start + 2),
        };
    }
    out
}

/// [RETURNED] scrap slots, always 4 bytes each.
fn returned(record: &[u8], at: usize) -> [Material; RETURNED] {
    let mut out = [Material::NONE; RETURNED];
    for (slot, entry) in out.iter_mut().enumerate() {
        let start = at + 4 * slot;
        *entry = Material {
            item: u16_at(record, start),
            count: u8_at(record, start + 2),
        };
    }
    out
}

/// One provision, `u16` group and `u8` value.
fn provision(record: &[u8], at: usize) -> Provision {
    Provision {
        group: u16_at(record, at),
        value: u8_at(record, at + 2),
    }
}

/// Read one table into recipes.
///
/// Returns an empty list for anything that is not the table its name claims - a
/// truncated dump, another container, or nothing at all.
pub fn parse(kind: TableKind, bytes: &[u8]) -> Vec<Recipe> {
    let Some(table) = records(bytes, kind.record_size()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for record in table.chunks_exact(kind.record_size()) {
        match kind {
            TableKind::WeaponCreate(class) => {
                let id = u16_at(record, 0x02);
                if id == 0 {
                    continue;
                }
                out.push(Recipe {
                    product: Product::Weapon { class, id },
                    level: FORGE,
                    materials: materials(record, 0x06, 4),
                    provision: provision(record, 0x16),
                    returned: returned(record, 0x1A),
                });
            }
            TableKind::WeaponProcess(class) => {
                let id = u16_at(record, 0x00);
                if id == 0 {
                    continue;
                }
                out.push(Recipe {
                    product: Product::Weapon { class, id },
                    level: u16_at(record, 0x02),
                    materials: materials(record, 0x04, 3),
                    provision: provision(record, 0x10),
                    returned: returned(record, 0x14),
                });
            }
            TableKind::ArmorCreate(part) => {
                for slot in 0..MATERIALS {
                    let series = u16_at(record, 2 * slot);
                    if series == 0 {
                        continue;
                    }
                    out.push(Recipe {
                        product: Product::Armor { series, part },
                        level: FORGE,
                        materials: materials(record, 0x0A, 4),
                        provision: provision(record, 0x1A),
                        returned: returned(record, 0x1E),
                    });
                }
            }
            TableKind::ArmorProcess => {
                let level = u16_at(record, 0x08);
                for slot in 0..MATERIALS {
                    let series = u16_at(record, 2 * slot);
                    if series == 0 {
                        continue;
                    }
                    out.push(Recipe {
                        product: Product::ArmorSeries { series },
                        level,
                        materials: materials(record, 0x0A, 3),
                        provision: provision(record, 0x16),
                        returned: returned(record, 0x1A),
                    });
                }
            }
            TableKind::DecoCreate => {
                let item = u16_at(record, 0x00);
                if item == 0 {
                    continue;
                }
                out.push(Recipe {
                    product: Product::Deco { item },
                    level: FORGE,
                    materials: materials(record, 0x04, 3),
                    provision: Provision::NONE,
                    returned: [Material::NONE; RETURNED],
                });
            }
        }
    }
    out
}

/// Every recipe of one dump, with both directions indexed.
#[derive(Debug, Clone, Default)]
pub struct Book {
    recipes: Vec<Recipe>,
    by_product: BTreeMap<Product, Vec<usize>>,
    by_item: BTreeMap<u16, Vec<usize>>,
    files: usize,
    skipped: usize,
}

impl Book {
    /// An empty book.
    pub fn new() -> Book {
        Book::default()
    }

    /// Read one file into the book, returning how many recipes it added.
    ///
    /// A file whose name is not one of the five is ignored silently (the
    /// `nativeNX/table` folder holds 214 others); a file that *is* named like one
    /// but does not pass the header check is counted in [Book::skipped], so a
    /// caller can say how much of the dump it could not read.
    pub fn add(&mut self, file: &str, bytes: &[u8]) -> usize {
        let Some(kind) = kind_of(file) else {
            return 0;
        };
        let parsed = parse(kind, bytes);
        if parsed.is_empty() {
            self.skipped += 1;
            return 0;
        }
        self.files += 1;
        let added = parsed.len();
        for recipe in parsed {
            let index = self.recipes.len();
            self.recipes.push(recipe);
            self.by_product
                .entry(recipe.product)
                .or_default()
                .push(index);
            for material in recipe.materials.iter().filter(|m| m.is_used()) {
                let list = self.by_item.entry(material.item).or_default();
                // One recipe that asks for the same item twice is still one
                // recipe: the reverse list is "what needs this", not "how
                // often".
                if list.last() != Some(&index) {
                    list.push(index);
                }
            }
        }
        added
    }

    /// How many recipes the book holds.
    pub fn len(&self) -> usize {
        self.recipes.len()
    }

    /// Whether the book is empty.
    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }

    /// How many files were read.
    pub fn files(&self) -> usize {
        self.files
    }

    /// How many files looked like a recipe table but did not read.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Every recipe, in file order.
    pub fn recipes(&self) -> &[Recipe] {
        &self.recipes
    }

    /// Every product that has at least one recipe, sorted.
    pub fn products(&self) -> Vec<Product> {
        self.by_product.keys().copied().collect()
    }

    /// Every item used as a material, with how many recipes ask for it,
    /// sorted by item id.
    pub fn materials(&self) -> Vec<(u16, usize)> {
        self.by_item
            .iter()
            .map(|(item, recipes)| (*item, recipes.len()))
            .collect()
    }

    /// What a product costs, cheapest level first.
    ///
    /// The forge row (level 0) comes first, then the upgrades in level order;
    /// two forge rows for one product (a decoration with two recipes) keep the
    /// order they have in the file.
    pub fn of_product(&self, product: Product) -> Vec<&Recipe> {
        let mut out: Vec<&Recipe> = self
            .by_product
            .get(&product)
            .map(|list| list.iter().filter_map(|i| self.recipes.get(*i)).collect())
            .unwrap_or_default();
        out.sort_by_key(|recipe| recipe.level);
        out
    }

    /// Every recipe that asks for an item, in file order.
    pub fn of_material(&self, item: u16) -> Vec<&Recipe> {
        self.by_item
            .get(&item)
            .map(|list| list.iter().filter_map(|i| self.recipes.get(*i)).collect())
            .unwrap_or_default()
    }

    /// One line for the status bar: recipes, products, materials and files.
    pub fn summary(&self) -> String {
        format!(
            "{} レシピ / {} 生産物 / {} 素材 / {} ファイル",
            self.recipes.len(),
            self.by_product.len(),
            self.by_item.len(),
            self.files
        )
    }
}
