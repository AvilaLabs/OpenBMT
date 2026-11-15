//! The counter's store, in memory. It lives as long as the page does.

use std::collections::BTreeMap;

use openbmt_counter::{Slot, Store};

/// What the counter stores: its slots and its signed record log.
#[derive(Debug, Default)]
pub struct MemStore {
    slots: BTreeMap<Slot, Vec<u8>>,
    /// The records in the order they were written.
    pub records: Vec<Vec<u8>>,
}

impl Store for MemStore {
    type Error = &'static str;

    fn put(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), &'static str> {
        self.slots.insert(slot, bytes.to_vec());
        Ok(())
    }

    fn delete(&mut self, slot: Slot) -> Result<(), &'static str> {
        self.slots.remove(&slot);
        Ok(())
    }

    fn append_record(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.records.push(bytes.to_vec());
        Ok(())
    }

    fn read_records(&self, from: u64, max: usize) -> Result<Vec<Vec<u8>>, &'static str> {
        let from = usize::try_from(from).unwrap_or(usize::MAX);
        Ok(self.records.iter().skip(from).take(max).cloned().collect())
    }
}
