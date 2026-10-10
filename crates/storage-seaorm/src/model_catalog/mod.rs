//! V2 model metadata, routing and base prices in separate relational tables.
//! Price tiers, group authorization and billing ledgers remain separate work.

mod managed;
pub(crate) mod models;
mod reconcile;
pub use managed::ManagedModelsRepository;
pub(crate) mod price_tiers;
pub(crate) mod prices;
pub(crate) mod routes;
pub use models::{CatalogModelRecord, ModelCatalogRepository};
pub use price_tiers::{CatalogPriceTierRecord, ModelPriceTiersRepository};
pub use prices::{CatalogPriceRecord, ModelPricesRepository};
pub(crate) use reconcile::reconcile_builtin_catalog;
pub use routes::{CatalogRouteRecord, ModelRoutesRepository};

use sha2::{Digest, Sha256};

/// A fixed-size comparison key avoids collation differences and oversized
/// composite indexes across SQLite, MySQL and PostgreSQL.
pub(crate) fn comparison_key(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
pub(crate) mod tests;
