//! Tests. Each test is named for the rule it proves (`nr_NN_...`). They use
//! loopback sockets only; none needs a Bitcoin node.

mod driver;
mod kit;
mod meta;
mod own_node;
mod own_node_core;
mod socks;
mod storage;
mod v2;
