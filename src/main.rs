//! Punto de entrada: analiza la linea de comandos y ejecuta el modo solicitado.

use abadia::camera::{tour_camera, tour_keys, Camera};
use abadia::config::{self, Config, Mode};
use abadia::renderer::{render, RenderSettings};
use abadia::scene::build_world;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::AtomicUsize;

fn main() -> ExitCode {
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    let config = match config::parse(&argumentos) {
        Ok(Some(c)) => c,
        Ok(None) => {
            print!("{}", config::AYUDA);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("\nUsa `abadia --help` para ver los modos y las opciones.");
            return ExitCode::FAILURE;
        }
    };

    let resultado = match config.mode {
        Mode::Textures => modo_texturas(&config),
        Mode::Render => modo_render(&config),
        Mode::NormalsCompare => modo_comparar_normales(&config),
        Mode::Tour => modo_recorrido(&config),
        Mode::Benchmark => modo_benchmark(&config),
        Mode::Window => modo_ventana(&config),
    };

    match resultado {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn modo_texturas(c: &Config) -> Result<(), String> {
    let escritos =
        abadia::texgen::generate_all(&c.assets).map_err(|e| format!("generando recursos: {e}"))?;
    let total: u64 = escritos.iter().map(|(_, n)| n).sum();
    for (nombre, bytes) in &escritos {
        println!("  {nombre:<28} {bytes:>8} bytes");
    }
    println!(
        "{} ficheros escritos en {} ({:.1} KiB en total)",
        escritos.len(),
        c.assets.display(),
        total as f64 / 1024.0
    );
    if let Some(dir) = &c.previews {
        let n = abadia::texgen::write_previews(&c.assets, dir, 6)
            .map_err(|e| format!("escribiendo laminas: {e}"))?;
        println!("{} laminas PNG en {}", n.len(), dir.display());
    }
    Ok(())
}

/// Carga el mundo e informa de lo que se ha construido.
fn preparar(c: &Config) -> Result<abadia::renderer::World, String> {
    let (mundo, avisos) = build_world(&c.assets, &config::scene_spec(c));
    for a in &avisos {
        eprintln!("aviso: {a}");
    }
    if !avisos.is_empty() {
        eprintln!(
            "aviso: faltan recursos; ejecuta `abadia textures` para regenerarlos en {}",
            c.assets.display()
        );
    }
    println!(
        "escena: {} celdas ocupadas de {}, {} grupos emisores, semilla {}",
        mundo.grid.solid_count(),
        mundo.grid.cell_count(),
        mundo.lighting.emitters.len(),
        c.seed
    );
    Ok(mundo)
}

/// Aplica a la camara el limite de no atravesar la escena.
fn encajar_camara(mundo: &abadia::renderer::World, mut camara: Camera) -> Camera {
    camara.enforce_outside(mundo.grid.bounds(), 1.5);
    camara
}

fn informar(reporte: &abadia::renderer::RenderReport, s: &RenderSettings) {
    let megarayos = reporte.primary_rays as f64 / 1e6;
    println!(
        "render: {}x{} px, {} muestras/px, profundidad {}, {} bloques, {} hilos",
        s.width, s.height, s.samples, s.max_depth, reporte.tiles, reporte.threads
    );
    println!(
        "tiempo: {:.2} s  ({:.2} Mrayos primarios, {:.2} Mrayos/s)",
        reporte.seconds,
        megarayos,
        megarayos / reporte.seconds.max(1e-9)
    );
}

fn modo_render(c: &Config) -> Result<(), String> {
    let mundo = preparar(c)?;
    let camara = encajar_camara(&mundo, c.camera);

    let progreso = AtomicUsize::new(0);
    let reporte = render(&mundo, &camara, &c.settings, None, Some(&progreso))
        .ok_or("el render se cancelo")?;
    informar(&reporte, &c.settings);

    let img = reporte.framebuffer.to_image(c.settings.exposure);
    img.save(&c.out)
        .map_err(|e| format!("guardando {}: {e}", c.out.display()))?;
    println!("imagen escrita en {}", c.out.display());
    Ok(())
}

fn modo_comparar_normales(c: &Config) -> Result<(), String> {
    let mundo = preparar(c)?;
    let camara = encajar_camara(&mundo, c.camera);
    let base = c.out.parent().unwrap_or(Path::new("."));
    let raiz = c
        .out
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("comparacion");

    for (activo, sufijo) in [(true, "con-mapas-normales"), (false, "sin-mapas-normales")] {
        let mut s = c.settings.clone();
        s.normal_maps = activo;
        let reporte = render(&mundo, &camara, &s, None, None).ok_or("el render se cancelo")?;
        informar(&reporte, &s);
        let ruta = base.join(format!("{raiz}-{sufijo}.png"));
        reporte
            .framebuffer
            .to_image(s.exposure)
            .save(&ruta)
            .map_err(|e| format!("guardando {}: {e}", ruta.display()))?;
        println!("imagen escrita en {}", ruta.display());
    }
    Ok(())
}

fn modo_recorrido(c: &Config) -> Result<(), String> {
    let mundo = preparar(c)?;
    let keys = tour_keys();
    std::fs::create_dir_all(&c.out).map_err(|e| format!("creando {}: {e}", c.out.display()))?;

    let inicio = std::time::Instant::now();
    for f in 0..c.frames {
        let t = if c.frames <= 1 {
            0.0
        } else {
            f as f64 / (c.frames - 1) as f64
        };
        let camara = encajar_camara(&mundo, tour_camera(&keys, t, c.camera.vfov));
        let reporte =
            render(&mundo, &camara, &c.settings, None, None).ok_or("el render se cancelo")?;
        let ruta = c.out.join(format!("frame_{f:04}.png"));
        reporte
            .framebuffer
            .to_image(c.settings.exposure)
            .save(&ruta)
            .map_err(|e| format!("guardando {}: {e}", ruta.display()))?;
        println!(
            "fotograma {:>4}/{}  {:.2} s  {}",
            f + 1,
            c.frames,
            reporte.seconds,
            ruta.display()
        );
    }
    println!(
        "{} fotogramas en {:.1} s, escritos en {}",
        c.frames,
        inicio.elapsed().as_secs_f64(),
        c.out.display()
    );
    Ok(())
}

fn modo_benchmark(c: &Config) -> Result<(), String> {
    let mundo = preparar(c)?;
    let camara = encajar_camara(&mundo, c.camera);
    let maximos = abadia::renderer::hilos_disponibles();

    println!();
    println!(
        "{:>6} {:>6} {:>5} {:>6} {:>7} {:>9} {:>11} {:>10}",
        "ancho", "alto", "spp", "prof", "hilos", "bloques", "tiempo (s)", "Mrayos/s"
    );
    println!("{}", "-".repeat(70));

    // Barrido de calidad a todos los hilos.
    for (w, h, spp, depth) in [
        (640, 360, 1, 3),
        (640, 360, 4, 5),
        (1280, 720, 1, 3),
        (1280, 720, 4, 5),
        (1280, 720, 9, 5),
        (1920, 1080, 9, 5),
    ] {
        let s = RenderSettings {
            width: w,
            height: h,
            samples: spp,
            max_depth: depth,
            threads: maximos,
            ..c.settings.clone()
        };
        let r = render(&mundo, &camara, &s, None, None).ok_or("el render se cancelo")?;
        imprimir_fila(&s, &r);
    }

    // Escalado con el numero de hilos, a calidad fija.
    println!();
    println!("escalado con los hilos, a 1280x720 con 4 muestras:");
    println!(
        "{:>6} {:>6} {:>5} {:>6} {:>7} {:>9} {:>11} {:>10}",
        "ancho", "alto", "spp", "prof", "hilos", "bloques", "tiempo (s)", "Mrayos/s"
    );
    println!("{}", "-".repeat(70));
    let mut hilos = 1;
    let mut base = None;
    while hilos <= maximos {
        let s = RenderSettings {
            width: 1280,
            height: 720,
            samples: 4,
            max_depth: 5,
            threads: hilos,
            ..c.settings.clone()
        };
        let r = render(&mundo, &camara, &s, None, None).ok_or("el render se cancelo")?;
        imprimir_fila(&s, &r);
        if hilos == 1 {
            base = Some(r.seconds);
        } else if let Some(b) = base {
            println!(
                "        aceleracion respecto de un hilo: {:.2}x",
                b / r.seconds
            );
        }
        hilos *= 2;
    }
    Ok(())
}

fn imprimir_fila(s: &RenderSettings, r: &abadia::renderer::RenderReport) {
    let mrayos = r.primary_rays as f64 / 1e6;
    println!(
        "{:>6} {:>6} {:>5} {:>6} {:>7} {:>9} {:>11.2} {:>10.2}",
        s.width,
        s.height,
        s.samples,
        s.max_depth,
        r.threads,
        r.tiles,
        r.seconds,
        mrayos / r.seconds.max(1e-9)
    );
}

#[cfg(windows)]
fn modo_ventana(c: &Config) -> Result<(), String> {
    let mundo = preparar(c)?;
    abadia::platform::run_window(mundo, c)
}

#[cfg(not(windows))]
fn modo_ventana(_c: &Config) -> Result<(), String> {
    Err(
        "la ventana interactiva usa las API nativas de Windows y este binario no se compilo \
         para Windows; usa el modo `render` o `tour`, que son independientes del sistema"
            .to_string(),
    )
}
