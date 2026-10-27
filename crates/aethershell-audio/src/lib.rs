//! Default-on local WASAPI shared-loopback audio spectrum analyzer.
//!
//! Enforces zero raw audio persistence (PRD [S08]) and adaptive 15/30/60 FPS presentation.

pub mod fft;
pub mod notification;
pub mod spectrum;
pub mod wasapi;

pub use fft::FftEngine;
pub use spectrum::AudioSpectrumProcessor;
pub use wasapi::{FpsMode, WasapiLoopbackCapture};
