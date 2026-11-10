//! Tests. Each test is named for the rule it proves (`ps_NN_...`).

// A test hands the same sale to several polls; cloning it into a slice reads
// better than `from_ref` there.
#![allow(clippy::cloned_ref_to_slice_refs)]

mod alerts;
mod connector;
mod facts;
mod kit;
mod matching;
mod meta;
mod model;
mod notice;
mod props;
mod reference;
mod storage;
