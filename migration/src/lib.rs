pub use sea_orm_migration::prelude::*;

mod m20260119_080116_create_mnemonics_table;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260119_080116_create_mnemonics_table::Migration)]
    }
}
