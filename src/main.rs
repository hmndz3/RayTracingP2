//! Punto de entrada: analiza la linea de comandos y ejecuta el modo solicitado.

use std::path::PathBuf;
use std::process::ExitCode;

const AYUDA: &str = "\
La Abadia del Eclipse: raytracer de CPU sobre un diorama de cubos texturizados.

USO:
  abadia <modo> [opciones]

MODOS:
  textures            Regenera las texturas, los mapas normales y el cubemap
                      dentro de assets/. Los ficheros se guardan en el
                      repositorio, asi que solo hace falta si se cambia el
                      generador.

OPCIONES GENERALES:
  --assets <dir>      Carpeta de recursos (por omision: assets)
  -h, --help          Muestra esta ayuda
";

fn main() -> ExitCode {
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    if argumentos.is_empty() || argumentos.iter().any(|a| a == "-h" || a == "--help") {
        print!("{AYUDA}");
        return ExitCode::SUCCESS;
    }

    let modo = argumentos[0].clone();
    let mut assets = PathBuf::from("assets");
    let mut i = 1;
    while i < argumentos.len() {
        match argumentos[i].as_str() {
            "--assets" => {
                i += 1;
                match argumentos.get(i) {
                    Some(v) => assets = PathBuf::from(v),
                    None => {
                        eprintln!("error: --assets necesita una ruta");
                        return ExitCode::FAILURE;
                    }
                }
            }
            otro => {
                eprintln!("error: opcion desconocida: {otro}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    match modo.as_str() {
        "textures" => match abadia::texgen::generate_all(&assets) {
            Ok(escritos) => {
                let total: u64 = escritos.iter().map(|(_, n)| n).sum();
                for (nombre, bytes) in &escritos {
                    println!("  {nombre:<28} {:>8} bytes", bytes);
                }
                println!(
                    "{} ficheros escritos en {} ({:.1} KiB en total)",
                    escritos.len(),
                    assets.display(),
                    total as f64 / 1024.0
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error generando recursos: {e}");
                ExitCode::FAILURE
            }
        },
        otro => {
            eprintln!("error: modo desconocido: {otro}");
            eprint!("{AYUDA}");
            ExitCode::FAILURE
        }
    }
}
