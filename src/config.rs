//! Analisis de la linea de comandos.
//!
//! Se implementa a mano, como todo lo demas. La gramatica es deliberadamente
//! plana: un modo seguido de opciones largas con valor, sin agrupaciones ni
//! formas cortas, porque es lo que hace que un error de escritura se pueda
//! senalar con un mensaje util en lugar de con un uso generico.

use crate::camera::Camera;
use crate::renderer::{hilos_disponibles, RenderSettings};
use std::path::PathBuf;

/// Modo de ejecucion pedido en la linea de comandos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Render a fichero.
    Render,
    /// Ventana interactiva.
    Window,
    /// Secuencia de fotogramas del recorrido de demostracion.
    Tour,
    /// Medicion de rendimiento.
    Benchmark,
    /// Regeneracion de los recursos graficos.
    Textures,
    /// Comparacion con y sin mapas normales.
    NormalsCompare,
}

impl Mode {
    fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "render" => Mode::Render,
            "window" => Mode::Window,
            "tour" => Mode::Tour,
            "benchmark" => Mode::Benchmark,
            "textures" => Mode::Textures,
            "normals-compare" => Mode::NormalsCompare,
            _ => return None,
        })
    }
}

/// Configuracion completa de una ejecucion.
#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub assets: PathBuf,
    pub out: PathBuf,
    pub seed: u64,
    pub camera: Camera,
    pub settings: RenderSettings,
    /// Fotogramas del recorrido.
    pub frames: usize,
    /// Carpeta donde escribir las laminas de textura.
    pub previews: Option<PathBuf>,
    /// Escala de la vista interactiva respecto de la ventana.
    pub preview_scale: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            mode: Mode::Render,
            assets: PathBuf::from("assets"),
            out: PathBuf::from("docs/images/abadia.png"),
            seed: crate::terrain::TerrainSpec::default().seed,
            camera: Camera::initial(),
            settings: RenderSettings::default(),
            frames: 120,
            previews: None,
            preview_scale: 3,
        }
    }
}

/// Texto de ayuda.
pub const AYUDA: &str = "\
La Abadia del Eclipse: raytracer de CPU sobre un diorama de cubos texturizados.

USO:
  abadia <modo> [opciones]

MODOS:
  render            Traza una imagen y la guarda en disco.
  window            Abre la ventana interactiva (solo Windows).
  tour              Escribe la secuencia de fotogramas del recorrido guiado.
  benchmark         Mide el rendimiento a varias resoluciones y muestras.
  textures          Regenera texturas, mapas normales y cubemap en assets/.
  normals-compare   Escribe dos imagenes iguales, con y sin mapas normales.

IMAGEN:
  --width <n>       Anchura en pixeles (por omision 1280)
  --height <n>      Altura en pixeles (por omision 720)
  --samples <n>     Muestras por pixel (por omision 9)
  --depth <n>       Profundidad maxima de recursion (por omision 5)
  --exposure <f>    Multiplicador de exposicion (por omision 1.0)
  --threads <n>     Hilos de render (por omision, los de la maquina)
  --tile <n>        Lado del bloque de pixeles (por omision 32)
  --no-normal-maps  Desactiva los mapas normales

CAMARA:
  --yaw <grados>    Azimut de la camara
  --pitch <grados>  Elevacion de la camara
  --distance <f>    Distancia al objetivo
  --fov <grados>    Campo de vision vertical

ESCENA:
  --seed <n>        Semilla del terreno procedural

SALIDA:
  --out <ruta>      Fichero o carpeta de salida
  --frames <n>      Fotogramas del recorrido (modo tour)
  --previews <dir>  Laminas PNG de las texturas (modo textures)
  --assets <dir>    Carpeta de recursos (por omision assets)
  --scale <n>       Division de resolucion de la vista interactiva

  -h, --help        Muestra esta ayuda
";

/// Error de analisis, con el argumento culpable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Analiza los argumentos, sin incluir el nombre del programa.
pub fn parse(args: &[String]) -> Result<Option<Config>, ParseError> {
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(None);
    }

    let mode = Mode::parse(&args[0])
        .ok_or_else(|| ParseError(format!("modo desconocido: {}", args[0])))?;
    let mut c = Config {
        mode,
        ..Config::default()
    };

    // Ayudas de conversion que informan del nombre de la opcion al fallar, que es
    // la unica parte del mensaje que le sirve a quien lo lee.
    fn valor<'a>(args: &'a [String], i: &mut usize, nombre: &str) -> Result<&'a str, ParseError> {
        *i += 1;
        args.get(*i)
            .map(|s| s.as_str())
            .ok_or_else(|| ParseError(format!("{nombre} necesita un valor")))
    }
    fn entero(v: &str, nombre: &str) -> Result<usize, ParseError> {
        v.parse()
            .map_err(|_| ParseError(format!("{nombre} espera un entero, no {v:?}")))
    }
    fn real(v: &str, nombre: &str) -> Result<f64, ParseError> {
        v.parse()
            .map_err(|_| ParseError(format!("{nombre} espera un numero, no {v:?}")))
    }

    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--width" => c.settings.width = entero(valor(args, &mut i, a)?, a)?,
            "--height" => c.settings.height = entero(valor(args, &mut i, a)?, a)?,
            "--samples" => c.settings.samples = entero(valor(args, &mut i, a)?, a)?,
            "--depth" => c.settings.max_depth = entero(valor(args, &mut i, a)?, a)?,
            "--threads" => c.settings.threads = entero(valor(args, &mut i, a)?, a)?,
            "--tile" => c.settings.tile = entero(valor(args, &mut i, a)?, a)?,
            "--exposure" => c.settings.exposure = real(valor(args, &mut i, a)?, a)?,
            "--no-normal-maps" => c.settings.normal_maps = false,
            "--yaw" => c.camera.yaw = real(valor(args, &mut i, a)?, a)?,
            "--pitch" => c.camera.pitch = real(valor(args, &mut i, a)?, a)?,
            "--distance" => c.camera.distance = real(valor(args, &mut i, a)?, a)?,
            "--fov" => c.camera.vfov = real(valor(args, &mut i, a)?, a)?,
            "--seed" => {
                let v = valor(args, &mut i, a)?;
                c.seed = v
                    .parse()
                    .map_err(|_| ParseError(format!("--seed espera un entero, no {v:?}")))?;
            }
            "--frames" => c.frames = entero(valor(args, &mut i, a)?, a)?,
            "--scale" => c.preview_scale = entero(valor(args, &mut i, a)?, a)?,
            "--out" => c.out = PathBuf::from(valor(args, &mut i, a)?),
            "--assets" => c.assets = PathBuf::from(valor(args, &mut i, a)?),
            "--previews" => c.previews = Some(PathBuf::from(valor(args, &mut i, a)?)),
            otro => return Err(ParseError(format!("opcion desconocida: {otro}"))),
        }
        i += 1;
    }

    validar(&mut c)?;
    Ok(Some(c))
}

/// Comprueba los limites y aplica los ajustes derivados.
fn validar(c: &mut Config) -> Result<(), ParseError> {
    if c.settings.width == 0 || c.settings.height == 0 {
        return Err(ParseError("la resolucion no puede ser cero".into()));
    }
    if c.settings.width > 16_384 || c.settings.height > 16_384 {
        return Err(ParseError("resolucion excesiva, el limite es 16384".into()));
    }
    if c.settings.samples == 0 {
        return Err(ParseError(
            "hacen falta al menos una muestra por pixel".into(),
        ));
    }
    if c.settings.tile == 0 {
        return Err(ParseError(
            "el bloque de pixeles no puede ser de lado cero".into(),
        ));
    }
    if c.settings.threads == 0 {
        c.settings.threads = hilos_disponibles();
    }
    if c.frames == 0 {
        return Err(ParseError(
            "el recorrido necesita al menos un fotograma".into(),
        ));
    }
    if c.preview_scale == 0 {
        return Err(ParseError("la escala de la vista no puede ser cero".into()));
    }
    if !(1.0..=170.0).contains(&c.camera.vfov) {
        return Err(ParseError(format!(
            "campo de vision fuera de rango: {}",
            c.camera.vfov
        )));
    }
    // Los topes del orbitador se aplican siempre, vengan de donde vengan los
    // valores: la camara no puede quedar invertida ni dentro de la escena.
    c.camera.clamp();
    Ok(())
}

/// Especificacion de escena derivada de la configuracion.
pub fn scene_spec(c: &Config) -> crate::scene::SceneSpec {
    crate::scene::SceneSpec::default().with_seed(c.seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn sin_argumentos_o_con_ayuda_no_hay_configuracion() {
        assert!(parse(&[]).unwrap().is_none());
        assert!(parse(&args(&["--help"])).unwrap().is_none());
        assert!(parse(&args(&["render", "-h"])).unwrap().is_none());
    }

    #[test]
    fn se_reconocen_todos_los_modos() {
        for (texto, modo) in [
            ("render", Mode::Render),
            ("window", Mode::Window),
            ("tour", Mode::Tour),
            ("benchmark", Mode::Benchmark),
            ("textures", Mode::Textures),
            ("normals-compare", Mode::NormalsCompare),
        ] {
            let c = parse(&args(&[texto])).unwrap().unwrap();
            assert_eq!(c.mode, modo);
        }
    }

    #[test]
    fn un_modo_desconocido_se_senala_por_su_nombre() {
        let e = parse(&args(&["renderizar"])).unwrap_err();
        assert!(e.0.contains("renderizar"), "{e}");
    }

    #[test]
    fn una_opcion_desconocida_se_senala_por_su_nombre() {
        let e = parse(&args(&["render", "--anchura", "800"])).unwrap_err();
        assert!(e.0.contains("--anchura"), "{e}");
    }

    #[test]
    fn una_opcion_sin_valor_se_detecta() {
        let e = parse(&args(&["render", "--width"])).unwrap_err();
        assert!(e.0.contains("--width") && e.0.contains("valor"), "{e}");
    }

    #[test]
    fn un_valor_no_numerico_se_detecta_con_su_opcion() {
        let e = parse(&args(&["render", "--samples", "muchas"])).unwrap_err();
        assert!(e.0.contains("--samples") && e.0.contains("muchas"), "{e}");
        let e = parse(&args(&["render", "--exposure", "clara"])).unwrap_err();
        assert!(e.0.contains("--exposure"), "{e}");
    }

    #[test]
    fn las_opciones_de_imagen_llegan_a_los_ajustes() {
        let c = parse(&args(&[
            "render",
            "--width",
            "640",
            "--height",
            "360",
            "--samples",
            "16",
            "--depth",
            "7",
            "--threads",
            "3",
            "--tile",
            "8",
            "--exposure",
            "1.4",
            "--no-normal-maps",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(c.settings.width, 640);
        assert_eq!(c.settings.height, 360);
        assert_eq!(c.settings.samples, 16);
        assert_eq!(c.settings.max_depth, 7);
        assert_eq!(c.settings.threads, 3);
        assert_eq!(c.settings.tile, 8);
        assert!((c.settings.exposure - 1.4).abs() < 1e-12);
        assert!(!c.settings.normal_maps);
    }

    #[test]
    fn las_opciones_de_camara_llegan_y_se_recortan() {
        let c = parse(&args(&[
            "render",
            "--yaw",
            "40",
            "--pitch",
            "250",
            "--distance",
            "3",
            "--fov",
            "55",
        ]))
        .unwrap()
        .unwrap();
        assert!((c.camera.yaw - 40.0).abs() < 1e-12);
        // La elevacion y la distancia se ajustan a los topes del orbitador.
        assert!(c.camera.pitch <= crate::camera::PITCH_MAX);
        assert!(c.camera.distance >= crate::camera::DISTANCE_MIN);
        assert!((c.camera.vfov - 55.0).abs() < 1e-12);
    }

    #[test]
    fn la_semilla_se_puede_cambiar_desde_la_linea_de_comandos() {
        let c = parse(&args(&["render", "--seed", "1234567"]))
            .unwrap()
            .unwrap();
        assert_eq!(c.seed, 1_234_567);
        assert_eq!(scene_spec(&c).terrain.seed, 1_234_567);
        // Y por omision se usa la del proyecto.
        let d = parse(&args(&["render"])).unwrap().unwrap();
        assert_eq!(d.seed, crate::terrain::TerrainSpec::default().seed);
    }

    #[test]
    fn se_rechazan_los_valores_imposibles() {
        for malo in [
            vec!["render", "--width", "0"],
            vec!["render", "--height", "0"],
            vec!["render", "--samples", "0"],
            vec!["render", "--tile", "0"],
            vec!["render", "--frames", "0"],
            vec!["render", "--scale", "0"],
            vec!["render", "--fov", "0"],
            vec!["render", "--fov", "200"],
            vec!["render", "--width", "99999"],
        ] {
            assert!(parse(&args(&malo)).is_err(), "deberia fallar: {malo:?}");
        }
    }

    #[test]
    fn cero_hilos_significa_los_de_la_maquina() {
        let c = parse(&args(&["render", "--threads", "0"]))
            .unwrap()
            .unwrap();
        assert_eq!(c.settings.threads, hilos_disponibles());
        assert!(c.settings.threads >= 1);
    }

    #[test]
    fn las_rutas_se_toman_tal_cual() {
        let c = parse(&args(&[
            "tour",
            "--out",
            "salida/recorrido",
            "--assets",
            "otros/recursos",
            "--previews",
            "docs/tex",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(c.out, PathBuf::from("salida/recorrido"));
        assert_eq!(c.assets, PathBuf::from("otros/recursos"));
        assert_eq!(c.previews, Some(PathBuf::from("docs/tex")));
    }

    #[test]
    fn la_ayuda_documenta_todos_los_modos_y_opciones() {
        for modo in [
            "render",
            "window",
            "tour",
            "benchmark",
            "textures",
            "normals-compare",
        ] {
            assert!(AYUDA.contains(modo), "la ayuda no menciona {modo}");
        }
        for opcion in [
            "--width",
            "--height",
            "--samples",
            "--depth",
            "--exposure",
            "--threads",
            "--tile",
            "--no-normal-maps",
            "--yaw",
            "--pitch",
            "--distance",
            "--fov",
            "--seed",
            "--out",
            "--frames",
            "--previews",
            "--assets",
            "--scale",
        ] {
            assert!(AYUDA.contains(opcion), "la ayuda no menciona {opcion}");
        }
    }
}
