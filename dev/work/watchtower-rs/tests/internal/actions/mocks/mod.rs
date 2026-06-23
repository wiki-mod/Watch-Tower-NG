#![forbid(unsafe_code)]
#![allow(unused_imports)]

//! Mock implementations for testing actions.

pub mod client;

pub use client::{MockClient, TestData};
