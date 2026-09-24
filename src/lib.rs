//! La Abadia del Eclipse: raytracer de CPU sobre un diorama de cubos texturizados.
//!
//! El proyecto completo se apoya unicamente en la biblioteca estandar de Rust.
//! No hay motores graficos, bibliotecas matematicas, lectores de imagen externos
//! ni computo en GPU: la rejilla voxel, las intersecciones, las texturas, el
//! sombreado, los formatos de imagen y el paralelismo estan implementados aqui.

pub mod geometry;
pub mod math;
pub mod ray;

/// Version del proyecto, tomada de Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
