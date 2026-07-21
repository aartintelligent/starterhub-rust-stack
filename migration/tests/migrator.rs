//! Guard test of the migration roster.

use migration::{Migrator, MigratorTrait};

/// The template ships with an empty roster: the list exists, compiles,
/// and is ready for the first `Box::new(source::...)` entry. Once a
/// migration ships, update this test to assert the roster only ever
/// grows — entries are append-only, never reordered or deleted.
#[test]
fn roster_is_empty_in_the_template() {
    assert!(Migrator::migrations().is_empty());
}
