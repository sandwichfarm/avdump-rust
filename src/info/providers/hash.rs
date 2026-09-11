//! Collects hash digests from all hash calculators.

use crate::info::meta::{container_types, keys, MetaProvider};
use crate::info::value::Value;
use crate::processing::consumers::{BlockConsumer, HashCalculator};

pub struct HashProvider;

impl HashProvider {
    pub const NAME: &'static str = "HashProvider";

    pub fn create(calculators: &[&HashCalculator]) -> MetaProvider {
        let mut p = MetaProvider::new(Self::NAME, container_types::HASH_PROVIDER);
        for calc in calculators {
            p.add(calc.name(), keys::DIMENSIONLESS, Value::Binary(calc.hash_value().to_vec()));
            for (i, extra) in calc.additional_hash_values().iter().enumerate() {
                p.add(&format!("{}{}", calc.name(), i + 2), keys::DIMENSIONLESS, Value::Binary(extra.clone()));
            }
        }
        p
    }
}
