//! Trazado recursivo, control de energia, mapeo tonal y render por bloques.
//!
//! # Reparto de energia
//!
//! En cada impacto la energia se reparte, no se suma sin control:
//!
//! - En un dielectrico transmisivo, Fresnel decide la fraccion `kr` que se
//!   refleja; lo que queda, `1 - kr`, se transmite multiplicado por la
//!   transparencia del material. Las dos ramas suman como mucho uno.
//! - En un dielectrico opaco, esa misma `kr` pondera el entorno reflejado y
//!   `1 - kr` la componente difusa.
//! - En un conductor no hay componente difusa: toda la energia se va por el
//!   reflejo, tenido por el color del metal.
//!
//! El realce especular directo se suma aparte, ponderado por el parametro
//! especular del material, que nunca llega a uno.

use crate::acceleration::VoxelGrid;
use crate::camera::{Camera, CameraBasis};
use crate::geometry::Hit;
use crate::image::Image;
use crate::lighting::{ambient_light, direct_light, LightContext, Lighting};
use crate::material::{MaterialSet, AIR};
use crate::math::{
    fresnel_schlick_dielectric, fresnel_schlick_f0, reflect, refract, v3, Onb, Rng, Vec3,
};
use crate::ray::{offset_ray, Interval, Medium, Ray};
use crate::skybox::Skybox;
use crate::texture::linear_to_srgb;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Todo lo que el trazador necesita para resolver un rayo.
pub struct World {
    pub grid: VoxelGrid,
    pub materials: MaterialSet,
    pub skybox: Skybox,
    pub lighting: Lighting,
}

/// Parametros de un render.
#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub width: usize,
    pub height: usize,
    /// Muestras por pixel. Se estratifican sobre una retícula cuadrada.
    pub samples: usize,
    /// Profundidad maxima de la recursion.
    pub max_depth: usize,
    /// Multiplicador de exposicion antes del mapeo tonal.
    pub exposure: f64,
    /// Si se aplican los mapas normales. Desactivarlo es el modo de comparacion.
    pub normal_maps: bool,
    pub threads: usize,
    /// Lado del bloque de pixeles que toma cada hilo.
    pub tile: usize,
}

impl Default for RenderSettings {
    fn default() -> RenderSettings {
        RenderSettings {
            width: 1280,
            height: 720,
            samples: 9,
            max_depth: 5,
            exposure: 1.0,
            normal_maps: true,
            threads: hilos_disponibles(),
            tile: 32,
        }
    }
}

impl RenderSettings {
    /// Ajustes de vista interactiva: resolucion reducida y una muestra por pixel.
    pub fn interactive(width: usize, height: usize) -> RenderSettings {
        RenderSettings {
            width,
            height,
            samples: 1,
            max_depth: 3,
            ..RenderSettings::default()
        }
    }
}

/// Numero de hilos que ofrece la maquina, con reserva por si no se puede saber.
pub fn hilos_disponibles() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Bloque de pixeles con su propio almacenamiento.
///
/// Cada bloque es duenno de sus pixeles, asi que repartirlos entre hilos no
/// necesita ni punteros crudos ni un bloqueo por pixel: basta con entregar cada
/// bloque a un hilo distinto.
#[derive(Debug, Clone)]
pub struct Tile {
    pub x0: usize,
    pub y0: usize,
    pub width: usize,
    pub height: usize,
    /// Radiancia lineal acumulada por pixel.
    pub pixels: Vec<Vec3>,
}

/// Imagen en curso, dividida en bloques.
#[derive(Debug, Clone)]
pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Tile>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize, tile: usize) -> Framebuffer {
        let tile = tile.max(1);
        let mut tiles = Vec::new();
        let mut y0 = 0;
        while y0 < height {
            let h = tile.min(height - y0);
            let mut x0 = 0;
            while x0 < width {
                let w = tile.min(width - x0);
                tiles.push(Tile {
                    x0,
                    y0,
                    width: w,
                    height: h,
                    pixels: vec![Vec3::ZERO; w * h],
                });
                x0 += w;
            }
            y0 += h;
        }
        Framebuffer {
            width,
            height,
            tiles,
        }
    }

    /// Vuelca los bloques a una imagen contigua, aplicando exposicion, mapeo
    /// tonal y codificacion a sRGB.
    pub fn to_image(&self, exposure: f64) -> Image {
        let mut img = Image::new(self.width, self.height);
        for t in &self.tiles {
            for y in 0..t.height {
                for x in 0..t.width {
                    let c = encode(t.pixels[y * t.width + x], exposure);
                    img.set(t.x0 + x, t.y0 + y, c);
                }
            }
        }
        img
    }

    /// Vuelca a un bufer de 32 bits en orden BGRA, que es el que espera la
    /// presentacion en pantalla de Windows.
    pub fn to_bgra(&self, exposure: f64, out: &mut [u32]) {
        for t in &self.tiles {
            for y in 0..t.height {
                let fila = (t.y0 + y) * self.width;
                for x in 0..t.width {
                    let [r, g, b] = encode(t.pixels[y * t.width + x], exposure);
                    out[fila + t.x0 + x] = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
                }
            }
        }
    }
}

/// Mapeo tonal y codificacion de un pixel.
#[inline]
fn encode(color: Vec3, exposure: f64) -> [u8; 3] {
    let c = tone_map(color * exposure);
    [
        (linear_to_srgb(c.x) * 255.0).round().clamp(0.0, 255.0) as u8,
        (linear_to_srgb(c.y) * 255.0).round().clamp(0.0, 255.0) as u8,
        (linear_to_srgb(c.z) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Curva tonal filmica con hombro.
///
/// Se prefiere a un simple recorte porque los faroles y el vitral superan con
/// mucho la unidad: recortar los volveria discos blancos de borde duro, mientras
/// que el hombro conserva el tono ambar hasta el nucleo. Es la aproximacion
/// racional habitual a la curva ACES.
#[inline]
pub fn tone_map(x: Vec3) -> Vec3 {
    #[inline]
    fn canal(v: f64) -> f64 {
        let v = v.max(0.0);
        ((v * (2.51 * v + 0.03)) / (v * (2.43 * v + 0.59) + 0.14)).clamp(0.0, 1.0)
    }
    v3(canal(x.x), canal(x.y), canal(x.z))
}

/// Estado del rayo dentro de la recursion.
#[derive(Debug, Clone, Copy)]
struct Viaje {
    medium: Medium,
    /// Material del medio, necesario para que el recorrido no vea interfaces
    /// falsas al atravesar celdas contiguas del mismo cuerpo.
    medium_material: u16,
    depth: usize,
}

/// Radiancia que llega por un rayo.
pub fn trace(world: &World, ray: &Ray, settings: &RenderSettings, rng: &mut Rng) -> Vec3 {
    let viaje = Viaje {
        medium: Medium::AIR,
        medium_material: AIR,
        depth: settings.max_depth,
    };
    trace_interno(world, ray, viaje, settings, rng)
}

fn trace_interno(
    world: &World,
    ray: &Ray,
    viaje: Viaje,
    settings: &RenderSettings,
    rng: &mut Rng,
) -> Vec3 {
    let Some(hit) = world
        .grid
        .hit(ray, Interval::positive(), viaje.medium_material)
    else {
        return world.skybox.sample(ray.dir);
    };

    // Absorcion del medio a lo largo del tramo recorrido hasta la superficie.
    let transmitancia = viaje.medium.transmittance(hit.t);
    if transmitancia.max_component() < 1e-4 {
        return Vec3::ZERO;
    }

    sombrear(world, ray, &hit, viaje, settings, rng).mul_elem(transmitancia)
}

fn sombrear(
    world: &World,
    ray: &Ray,
    hit: &Hit,
    viaje: Viaje,
    settings: &RenderSettings,
    rng: &mut Rng,
) -> Vec3 {
    let m = world.materials.get(hit.material);
    let textura = world.materials.albedo_at(m, hit);
    let normal = world.materials.shading_normal(m, hit, settings.normal_maps);
    let vista = -ray.dir;
    let cos_i = vista.dot(normal).clamp(0.0, 1.0);

    // Emision propia. Se modula por la textura para que el farol tenga nucleo y
    // celosia en lugar de ser un cubo de color plano.
    let mut color = if m.is_emissive() {
        m.emission.mul_elem(textura)
    } else {
        Vec3::ZERO
    };

    let ctx = LightContext {
        grid: &world.grid,
        materials: &world.materials,
        skybox: &world.skybox,
        lighting: &world.lighting,
    };

    if m.metallic {
        // Conductor: sin difusa. La reflectancia normal es el propio color del
        // metal, asi que el reflejo sale tenido de bronce.
        let f0 = textura * m.reflectivity;
        let f = fresnel_schlick_f0(cos_i, f0);
        let reflejo = if viaje.depth > 0 {
            let dir = direccion_reflejada(ray.dir, normal, m.shininess, rng);
            let r = offset_ray(hit.point, hit.normal, dir);
            trace_interno(
                world,
                &r,
                Viaje {
                    depth: viaje.depth - 1,
                    ..viaje
                },
                settings,
                rng,
            )
        } else {
            world.skybox.sample(reflect(ray.dir, normal))
        };
        color += f.mul_elem(reflejo);

        let luz = direct_light(&ctx, hit, normal, vista, m, viaje.medium_material, rng);
        color += luz.specular.mul_elem(f0) * m.specular;
        return color;
    }

    if m.is_transmissive() {
        // Indices a cada lado de la interfaz. El del medio actual se arrastra por
        // la recursion, de modo que entrar y salir quedan bien distinguidos.
        let n1 = viaje.medium.ior;
        let n2 = if hit.front_face { m.ior } else { 1.0 };
        let eta = n1 / n2;

        let refractada = refract(ray.dir, normal, eta);
        // Sin solucion real hay reflexion interna total: toda la energia vuelve.
        let kr = match refractada {
            None => 1.0,
            Some(_) => fresnel_schlick_dielectric(cos_i, n1, n2),
        };

        if viaje.depth > 0 {
            if kr > 1e-3 {
                let dir = direccion_reflejada(ray.dir, normal, m.shininess, rng);
                let r = offset_ray(hit.point, hit.normal, dir);
                color += trace_interno(
                    world,
                    &r,
                    Viaje {
                        depth: viaje.depth - 1,
                        ..viaje
                    },
                    settings,
                    rng,
                ) * kr;
            }
            if let Some(dir) = refractada {
                let peso = (1.0 - kr) * m.transparency;
                if peso > 1e-3 {
                    // Al entrar se adopta el medio del material; al salir, el aire.
                    let (medium, medium_material) = if hit.front_face {
                        (m.medium(textura), hit.material)
                    } else {
                        (Medium::AIR, AIR)
                    };
                    let r = offset_ray(hit.point, hit.normal, dir);
                    color += trace_interno(
                        world,
                        &r,
                        Viaje {
                            medium,
                            medium_material,
                            depth: viaje.depth - 1,
                        },
                        settings,
                        rng,
                    ) * peso;
                }
            }
        } else {
            // Agotada la profundidad, el entorno cierra las dos ramas.
            color += world.skybox.sample(reflect(ray.dir, normal)) * kr;
            if let Some(dir) = refractada {
                color += world.skybox.sample(dir) * ((1.0 - kr) * m.transparency);
            }
        }

        let luz = direct_light(&ctx, hit, normal, vista, m, viaje.medium_material, rng);
        color += luz.specular * (m.specular * kr);
        return color;
    }

    // Dielectrico opaco: difusa mas un reflejo de entorno debil que solo se nota
    // en incidencia rasante, que es como se comporta la piedra humeda.
    let kr = fresnel_schlick_dielectric(cos_i, 1.0, ior_desde_reflectancia(m.reflectivity));
    let luz = direct_light(&ctx, hit, normal, vista, m, viaje.medium_material, rng);
    let ambiente = ambient_light(&ctx, hit, normal);

    color += textura.mul_elem(luz.diffuse + ambiente) * (1.0 - kr);
    color += luz.specular * m.specular;
    if m.reflectivity > 0.0 {
        // Una sola consulta al cubemap, sin recursion: para la piedra y la madera
        // un rayo reflejado costaria tanto como el primario y apenas cambiaria el
        // pixel.
        color += world.skybox.sample(reflect(ray.dir, normal)) * kr;
    }
    color
}

/// Indice de refraccion equivalente a una reflectancia normal dada.
///
/// Invierte `F0 = ((n - 1) / (n + 1))^2`, de modo que el parametro del material
/// se puede declarar como reflectancia, que es lo intuitivo, y Schlick sigue
/// recibiendo indices reales.
#[inline]
fn ior_desde_reflectancia(f0: f64) -> f64 {
    let f0 = f0.clamp(0.0, 0.99);
    let r = f0.sqrt();
    (1.0 + r) / (1.0 - r)
}

/// Direccion reflejada, con dispersion segun el exponente especular.
///
/// Un exponente alto deja el reflejo practicamente especular; uno bajo lo abre en
/// un lobulo, que es lo que separa el agua en calma del bronce picado. Se muestrea
/// el lobulo de Phong alrededor de la reflexion ideal.
fn direccion_reflejada(incidente: Vec3, normal: Vec3, shininess: f64, rng: &mut Rng) -> Vec3 {
    let ideal = reflect(incidente, normal);
    if shininess >= 2000.0 {
        return ideal;
    }
    let base = Onb::from_normal(ideal);
    let cos_theta = rng.next_f64().powf(1.0 / (shininess + 1.0));
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let phi = std::f64::consts::TAU * rng.next_f64();
    let dir = base.to_world(v3(sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta));
    // Si la dispersion empujase el rayo por debajo de la superficie, se usa la
    // reflexion ideal: es preferible a dejarlo entrar en el propio cuerpo.
    if dir.dot(normal) <= 1e-4 {
        ideal
    } else {
        dir
    }
}

/// Resultado de un render completo.
#[derive(Debug)]
pub struct RenderReport {
    pub framebuffer: Framebuffer,
    pub seconds: f64,
    pub tiles: usize,
    pub threads: usize,
    /// Rayos primarios trazados.
    pub primary_rays: u64,
}

/// Renderiza una vista completa repartiendo los bloques entre hilos.
///
/// El reparto es dinamico: cada hilo toma el siguiente bloque libre en cuanto
/// termina el suyo, de modo que los bloques caros, los del estanque y el vitral,
/// no dejan al resto esperando. El unico punto de sincronizacion es la entrega de
/// un bloque, no la escritura de un pixel, y cada hilo escribe solo dentro del
/// bloque que tiene en la mano.
///
/// Devuelve `None` si `cancel` se activa antes de terminar.
pub fn render(
    world: &World,
    camera: &Camera,
    settings: &RenderSettings,
    cancel: Option<&AtomicBool>,
    progress: Option<&AtomicUsize>,
) -> Option<RenderReport> {
    let inicio = std::time::Instant::now();
    let mut fb = Framebuffer::new(settings.width, settings.height, settings.tile);
    let basis = camera.basis(settings.width, settings.height);
    let hilos = settings.threads.clamp(1, 256).min(fb.tiles.len().max(1));
    let total_tiles = fb.tiles.len();

    let lado = (settings.samples as f64).sqrt().ceil().max(1.0) as usize;
    let muestras = settings.samples.max(1);

    let cancelado = AtomicBool::new(false);
    let rayos = AtomicU64Compat::new();

    {
        let pendientes = Mutex::new(fb.tiles.iter_mut());
        std::thread::scope(|scope| {
            for _ in 0..hilos {
                scope.spawn(|| {
                    let mut locales = 0u64;
                    loop {
                        if cancelado.load(Ordering::Relaxed) {
                            break;
                        }
                        if let Some(c) = cancel {
                            if c.load(Ordering::Relaxed) {
                                cancelado.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                        // Unica toma del cerrojo por bloque.
                        let siguiente = {
                            let mut it = pendientes.lock().unwrap();
                            it.next()
                        };
                        let Some(tile) = siguiente else { break };
                        locales += render_tile(tile, world, &basis, settings, lado, muestras);
                        if let Some(p) = progress {
                            p.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    rayos.add(locales);
                });
            }
        });
    }

    if cancelado.load(Ordering::Relaxed) {
        return None;
    }

    Some(RenderReport {
        framebuffer: fb,
        seconds: inicio.elapsed().as_secs_f64(),
        tiles: total_tiles,
        threads: hilos,
        primary_rays: rayos.get(),
    })
}

/// Contador compartido de 64 bits.
///
/// `AtomicU64` no esta garantizado en todas las plataformas que admite Rust, asi
/// que se acumula por hilo y solo se suma una vez al terminar el bloque de
/// trabajo, con un cerrojo que se toma tantas veces como hilos haya.
struct AtomicU64Compat {
    valor: Mutex<u64>,
}

impl AtomicU64Compat {
    fn new() -> AtomicU64Compat {
        AtomicU64Compat {
            valor: Mutex::new(0),
        }
    }
    fn add(&self, n: u64) {
        *self.valor.lock().unwrap() += n;
    }
    fn get(&self) -> u64 {
        *self.valor.lock().unwrap()
    }
}

/// Renderiza un bloque y devuelve cuantos rayos primarios ha trazado.
fn render_tile(
    tile: &mut Tile,
    world: &World,
    basis: &CameraBasis,
    settings: &RenderSettings,
    lado: usize,
    muestras: usize,
) -> u64 {
    // Semilla derivada de la posicion del bloque: el resultado no depende de que
    // hilo lo haya tomado, asi que dos ejecuciones dan la misma imagen.
    let mut rng = Rng::new(
        (tile.y0 as u64) << 32 | tile.x0 as u64 ^ (settings.samples as u64).wrapping_mul(0x9E37),
    );
    let mut trazados = 0u64;

    for y in 0..tile.height {
        for x in 0..tile.width {
            let px = (tile.x0 + x) as f64;
            let py = (tile.y0 + y) as f64;
            let mut suma = Vec3::ZERO;

            for s in 0..muestras {
                // Estratificacion sobre la retícula del pixel: reparte las
                // muestras en lugar de dejarlas agruparse al azar.
                let sx = s % lado;
                let sy = (s / lado) % lado;
                let jx = (sx as f64 + rng.next_f64()) / lado as f64;
                let jy = (sy as f64 + rng.next_f64()) / lado as f64;
                let r = basis.ray(px + jx, py + jy);
                suma += trace(world, &r, settings, &mut rng);
                trazados += 1;
            }

            let color = suma / muestras as f64;
            // Cortafuegos contra valores no finitos: un solo NaN se propagaria a
            // toda la imagen al promediar.
            tile.pixels[y * tile.width + x] = if color.is_finite() { color } else { Vec3::ZERO };
        }
    }
    trazados
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lighting::Lighting;
    use crate::material::{LANTERN, STONE_ANCIENT, WATER};
    use crate::skybox::Skybox;
    use std::path::PathBuf;

    fn assets() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
    }

    fn mundo_de_prueba(construir: impl Fn(&mut VoxelGrid)) -> World {
        let (materials, avisos) = MaterialSet::load(&assets());
        assert!(avisos.is_empty(), "faltan recursos: {avisos:?}");
        let mut grid = VoxelGrid::new(24, 24, 24);
        construir(&mut grid);
        let emisores = Lighting::collect_emitters(&grid, &materials);
        World {
            grid,
            materials,
            skybox: Skybox::fallback(),
            lighting: Lighting::dusk(emisores),
        }
    }

    fn ajustes() -> RenderSettings {
        RenderSettings {
            width: 48,
            height: 32,
            samples: 1,
            max_depth: 4,
            threads: 2,
            tile: 16,
            ..RenderSettings::default()
        }
    }

    #[test]
    fn el_mapeo_tonal_es_monotono_y_no_se_desborda() {
        let mut anterior = -1.0;
        for i in 0..=2000 {
            let v = i as f64 * 0.05;
            let c = tone_map(Vec3::splat(v));
            assert!(c.x >= anterior - 1e-12, "la curva debe crecer");
            assert!((0.0..=1.0).contains(&c.x), "fuera de rango: {}", c.x);
            anterior = c.x;
        }
        assert_eq!(tone_map(Vec3::ZERO), Vec3::ZERO);
        // Valores muy altos se acercan a uno sin cortarse de golpe.
        assert!(tone_map(Vec3::splat(50.0)).x > 0.95);
        // Y conserva el tono: un color ambar sigue siendo ambar tras el hombro.
        let ambar = tone_map(v3(9.0, 5.5, 2.5));
        assert!(ambar.x > ambar.y && ambar.y > ambar.z);
    }

    #[test]
    fn el_mapeo_tonal_no_subexpone_los_valores_medios() {
        // Una superficie de reflectancia media bajo luz unidad debe caer en la
        // zona media de la imagen, no en las sombras.
        let salida = linear_to_srgb(tone_map(Vec3::splat(0.35)).x);
        assert!(salida > 0.45, "demasiado oscuro: {salida}");
        assert!(salida < 0.85, "demasiado claro: {salida}");
    }

    #[test]
    fn la_reflectancia_y_el_indice_son_inversos() {
        for f0 in [0.0, 0.02, 0.04, 0.2, 0.5] {
            let n = ior_desde_reflectancia(f0);
            let vuelta = ((n - 1.0) / (n + 1.0)).powi(2);
            assert!((vuelta - f0).abs() < 1e-9, "{f0} -> {n} -> {vuelta}");
            assert!(n >= 1.0);
        }
    }

    #[test]
    fn la_dispersion_del_reflejo_respeta_el_exponente() {
        let mut rng = Rng::new(5);
        let n = v3(0.0, 1.0, 0.0);
        let d = v3(0.4, -1.0, 0.2).normalized();
        let ideal = reflect(d, n);

        let desvio_medio = |shininess: f64, rng: &mut Rng| {
            let mut suma = 0.0;
            for _ in 0..2000 {
                let r = direccion_reflejada(d, n, shininess, rng);
                assert!(r.dot(n) > 0.0, "el reflejo entro en la superficie");
                assert!((r.length() - 1.0).abs() < 1e-9);
                suma += r.dot(ideal).clamp(-1.0, 1.0).acos();
            }
            suma / 2000.0
        };
        let afilado = desvio_medio(320.0, &mut rng);
        let ancho = desvio_medio(20.0, &mut rng);
        assert!(ancho > afilado * 2.0, "afilado {afilado}, ancho {ancho}");
        assert!(
            afilado.to_degrees() < 6.0,
            "el agua deberia ser casi espejo"
        );
    }

    #[test]
    fn un_rayo_al_vacio_devuelve_el_cielo() {
        let mundo = mundo_de_prueba(|_| {});
        let mut rng = Rng::new(1);
        let r = Ray::new(v3(12.0, 12.0, -10.0), v3(0.0, 0.3, -1.0));
        let c = trace(&mundo, &r, &ajustes(), &mut rng);
        assert_eq!(c, mundo.skybox.sample(r.dir));
    }

    #[test]
    fn una_superficie_iluminada_no_sale_negra() {
        let mundo = mundo_de_prueba(|g| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 5, k, STONE_ANCIENT);
                }
            }
        });
        let mut rng = Rng::new(2);
        let r = Ray::new(v3(12.5, 20.0, 12.5), v3(0.0, -1.0, 0.0));
        let c = trace(&mundo, &r, &ajustes(), &mut rng);
        assert!(c.luminance() > 0.02, "la piedra sale negra: {c:?}");
        assert!(c.is_finite());
    }

    #[test]
    fn el_material_emisivo_brilla_por_si_mismo() {
        let mundo = mundo_de_prueba(|g| {
            g.set(12, 12, 12, LANTERN);
        });
        let mut rng = Rng::new(3);
        // Se promedia la cara entera: la textura del farol tiene celosia oscura y
        // un solo punto podria caer justo sobre un travesano.
        let mut suma = Vec3::ZERO;
        let mut n = 0;
        for i in 0..16 {
            for j in 0..16 {
                let x = 12.0 + (i as f64 + 0.5) / 16.0;
                let y = 12.0 + (j as f64 + 0.5) / 16.0;
                let r = Ray::new(v3(x, y, 0.0), v3(0.0, 0.0, 1.0));
                suma += trace(&mundo, &r, &ajustes(), &mut rng);
                n += 1;
            }
        }
        let c = suma / n as f64;
        assert!(
            c.luminance() > 1.0,
            "un emisor debe superar la unidad: {c:?}"
        );
        assert!(c.x > c.z, "y ser calido");
        // El nucleo tiene que ser claramente mas brillante que la celosia.
        let mut rng2 = Rng::new(4);
        let nucleo = trace(
            &mundo,
            &Ray::new(v3(12.78, 12.78, 0.0), v3(0.0, 0.0, 1.0)),
            &ajustes(),
            &mut rng2,
        );
        let travesano = trace(
            &mundo,
            &Ray::new(v3(12.5, 12.5, 0.0), v3(0.0, 0.0, 1.0)),
            &ajustes(),
            &mut rng2,
        );
        assert!(nucleo.luminance() > travesano.luminance() * 2.0);
    }

    #[test]
    fn el_emisor_ilumina_lo_que_tiene_al_lado() {
        // Misma escena con y sin farol: la diferencia sobre el suelo es la
        // aportacion del muestreo explicito del emisor.
        let suelo = |g: &mut VoxelGrid| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 5, k, STONE_ANCIENT);
                }
            }
        };
        let sin = mundo_de_prueba(suelo);
        let con = mundo_de_prueba(|g| {
            suelo(g);
            g.set(12, 8, 12, LANTERN);
        });
        assert_eq!(con.lighting.emitters.len(), 1);

        let mut ajustes = ajustes();
        ajustes.max_depth = 2;
        let medir = |mundo: &World, semilla: u64| {
            let mut rng = Rng::new(semilla);
            let mut suma = 0.0;
            // Puntos del suelo alrededor del farol, evitando el propio bloque.
            for (dx, dz) in [(2.5, 0.0), (-2.5, 0.0), (0.0, 2.5), (0.0, -2.5)] {
                let p = v3(12.5 + dx, 20.0, 12.5 + dz);
                let r = Ray::new(p, v3(0.0, -1.0, 0.0));
                suma += trace(mundo, &r, &ajustes, &mut rng).luminance();
            }
            suma / 4.0
        };
        let a = medir(&sin, 11);
        let b = medir(&con, 11);
        assert!(b > a * 1.15, "el farol no ilumina el suelo: {a} -> {b}");
    }

    #[test]
    fn el_agua_refracta_y_desplaza_lo_que_hay_debajo() {
        // Fondo con dos bloques de material distinto bajo el agua. Mirando en
        // oblicuo, el rayo refractado debe caer sobre un bloque distinto del que
        // veria en linea recta.
        let construir = |con_agua: bool| {
            move |g: &mut VoxelGrid| {
                for i in 0..24 {
                    for k in 0..24 {
                        g.set(i, 4, k, STONE_ANCIENT);
                    }
                }
                if con_agua {
                    for i in 0..24 {
                        for k in 0..24 {
                            for j in 5..8 {
                                g.set(i, j, k, WATER);
                            }
                        }
                    }
                }
            }
        };
        let seco = mundo_de_prueba(construir(false));
        let mojado = mundo_de_prueba(construir(true));

        // Rayo muy oblicuo hacia el fondo.
        let origen = v3(4.0, 14.0, 12.5);
        let dir = v3(0.90, -0.435, 0.0);
        let r = Ray::new(origen, dir);

        let golpe_seco = seco
            .grid
            .hit(&r, Interval::positive(), AIR)
            .expect("debe ver el fondo");
        // En el mundo con agua, el primer impacto es la superficie.
        let superficie = mojado
            .grid
            .hit(&r, Interval::positive(), AIR)
            .expect("debe ver el agua");
        assert_eq!(superficie.material, WATER);

        // Se calcula a mano el punto al que llega el rayo refractado.
        let m = mojado.materials.get(WATER);
        let n = superficie.facing_normal();
        let refractado = refract(r.dir, n, 1.0 / m.ior).expect("no deberia haber reflexion total");
        let dentro = Ray::new(superficie.point + refractado * 1e-4, refractado);
        let fondo = mojado
            .grid
            .hit(&dentro, Interval::positive(), WATER)
            .expect("debe alcanzar el fondo");

        let desplazamiento = (fondo.point - golpe_seco.point).length();
        assert!(
            desplazamiento > 0.5,
            "el agua no desplaza la imagen del fondo: {desplazamiento}"
        );
        // Y el desplazamiento va en el sentido correcto: el rayo se endereza al
        // entrar en el medio denso, asi que avanza menos en horizontal.
        assert!(fondo.point.x < golpe_seco.point.x);
    }

    #[test]
    fn el_agua_tine_de_azul_lo_que_se_ve_a_traves() {
        let mundo = mundo_de_prueba(|g| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 2, k, STONE_ANCIENT);
                    for j in 3..9 {
                        g.set(i, j, k, WATER);
                    }
                }
            }
        });
        let mut rng = Rng::new(9);
        let r = Ray::new(v3(12.5, 20.0, 12.5), v3(0.0, -1.0, 0.0));
        let c = trace(&mundo, &r, &ajustes(), &mut rng);
        assert!(c.is_finite() && c.luminance() > 0.0);
        assert!(
            c.z > c.x,
            "seis bloques de agua deberian tenir de azul: {c:?}"
        );
    }

    #[test]
    fn el_render_completo_produce_una_imagen_razonable() {
        let mundo = mundo_de_prueba(|g| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 5, k, STONE_ANCIENT);
                }
            }
            for j in 6..12 {
                for k in 8..16 {
                    g.set(16, j, k, STONE_ANCIENT);
                }
            }
            g.set(10, 7, 10, LANTERN);
        });
        let ajustes = ajustes();
        let camara = Camera::initial();
        let reporte = render(&mundo, &camara, &ajustes, None, None).expect("no deberia cancelarse");

        assert_eq!(reporte.framebuffer.width, ajustes.width);
        assert_eq!(reporte.framebuffer.height, ajustes.height);
        assert_eq!(
            reporte.primary_rays,
            (ajustes.width * ajustes.height * ajustes.samples) as u64
        );

        let img = reporte.framebuffer.to_image(1.0);
        assert_eq!(img.data.len(), ajustes.width * ajustes.height * 3);

        // Ni todo negro ni todo blanco: la imagen tiene que tener rango.
        let mut minimo = 255u8;
        let mut maximo = 0u8;
        let mut suma = 0u64;
        for px in img.data.chunks(3) {
            let l = px[0].max(px[1]).max(px[2]);
            minimo = minimo.min(l);
            maximo = maximo.max(l);
            suma += px[0] as u64 + px[1] as u64 + px[2] as u64;
        }
        let media = suma as f64 / img.data.len() as f64;
        assert!(maximo > 60, "la imagen esta subexpuesta: maximo {maximo}");
        assert!(
            media > 20.0,
            "la imagen esta demasiado oscura: media {media}"
        );
        assert!(media < 235.0, "la imagen esta quemada: media {media}");
        assert!(maximo - minimo > 40, "no hay rango tonal");
    }

    #[test]
    fn el_render_es_reproducible_entre_ejecuciones_y_entre_repartos() {
        let mundo = mundo_de_prueba(|g| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 5, k, STONE_ANCIENT);
                }
            }
            g.set(10, 7, 10, LANTERN);
        });
        let camara = Camera::initial();

        let mut a = ajustes();
        a.threads = 1;
        let mut b = ajustes();
        b.threads = 8;

        let uno = render(&mundo, &camara, &a, None, None).unwrap();
        let otro = render(&mundo, &camara, &b, None, None).unwrap();
        // El reparto dinamico no debe alterar el resultado: cada bloque siembra su
        // generador con su propia posicion, no con el hilo que lo toma.
        assert_eq!(
            uno.framebuffer.to_image(1.0),
            otro.framebuffer.to_image(1.0)
        );
    }

    #[test]
    fn el_render_se_puede_cancelar() {
        let mundo = mundo_de_prueba(|g| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 5, k, STONE_ANCIENT);
                }
            }
        });
        let cancel = AtomicBool::new(true);
        let r = render(&mundo, &Camera::initial(), &ajustes(), Some(&cancel), None);
        assert!(r.is_none(), "un render cancelado no debe devolver imagen");
    }

    #[test]
    fn el_progreso_cuenta_todos_los_bloques() {
        let mundo = mundo_de_prueba(|_| {});
        let progreso = AtomicUsize::new(0);
        let r = render(
            &mundo,
            &Camera::initial(),
            &ajustes(),
            None,
            Some(&progreso),
        )
        .unwrap();
        assert_eq!(progreso.load(Ordering::Relaxed), r.tiles);
        assert!(r.tiles > 1, "deberia haber varios bloques");
    }

    #[test]
    fn los_bloques_cubren_la_imagen_exactamente_una_vez() {
        for (w, h, t) in [(48, 32, 16), (50, 33, 16), (7, 3, 4), (1, 1, 32)] {
            let fb = Framebuffer::new(w, h, t);
            let mut visitas = vec![0u32; w * h];
            for tile in &fb.tiles {
                assert_eq!(tile.pixels.len(), tile.width * tile.height);
                for y in 0..tile.height {
                    for x in 0..tile.width {
                        visitas[(tile.y0 + y) * w + tile.x0 + x] += 1;
                    }
                }
            }
            assert!(
                visitas.iter().all(|&v| v == 1),
                "reparto incorrecto para {w}x{h} con bloque {t}"
            );
        }
    }

    #[test]
    fn el_volcado_a_bgra_coincide_con_la_imagen() {
        let mundo = mundo_de_prueba(|g| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 5, k, STONE_ANCIENT);
                }
            }
        });
        let a = ajustes();
        let r = render(&mundo, &Camera::initial(), &a, None, None).unwrap();
        let img = r.framebuffer.to_image(1.3);
        let mut bgra = vec![0u32; a.width * a.height];
        r.framebuffer.to_bgra(1.3, &mut bgra);
        for y in 0..a.height {
            for x in 0..a.width {
                let [red, green, blue] = img.get(x, y);
                let v = bgra[y * a.width + x];
                assert_eq!((v >> 16) & 0xFF, red as u32);
                assert_eq!((v >> 8) & 0xFF, green as u32);
                assert_eq!(v & 0xFF, blue as u32);
            }
        }
    }

    #[test]
    fn desactivar_los_mapas_normales_cambia_la_imagen_pero_no_la_rompe() {
        let mundo = mundo_de_prueba(|g| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 5, k, STONE_ANCIENT);
                }
            }
            for j in 6..14 {
                for k in 6..18 {
                    g.set(17, j, k, STONE_ANCIENT);
                }
            }
        });
        let mut con = ajustes();
        con.samples = 4;
        let mut sin = con.clone();
        sin.normal_maps = false;

        let a = render(&mundo, &Camera::initial(), &con, None, None).unwrap();
        let b = render(&mundo, &Camera::initial(), &sin, None, None).unwrap();
        let ia = a.framebuffer.to_image(1.0);
        let ib = b.framebuffer.to_image(1.0);
        assert_ne!(ia, ib, "el mapa normal deberia notarse");

        let distintos = ia
            .data
            .iter()
            .zip(&ib.data)
            .filter(|(x, y)| x.abs_diff(**y) > 2)
            .count();
        let fraccion = distintos as f64 / ia.data.len() as f64;
        assert!(fraccion > 0.02, "el relieve apenas se nota: {fraccion:.3}");
    }

    #[test]
    fn ninguna_profundidad_produce_valores_no_finitos() {
        let mundo = mundo_de_prueba(|g| {
            for i in 0..24 {
                for k in 0..24 {
                    g.set(i, 4, k, STONE_ANCIENT);
                    for j in 5..8 {
                        g.set(i, j, k, WATER);
                    }
                }
            }
            g.set(12, 10, 12, LANTERN);
        });
        for depth in 0..7 {
            let mut a = ajustes();
            a.max_depth = depth;
            let r = render(&mundo, &Camera::initial(), &a, None, None).unwrap();
            for t in &r.framebuffer.tiles {
                for p in &t.pixels {
                    assert!(p.is_finite(), "profundidad {depth} produjo {p:?}");
                    assert!(p.x >= 0.0 && p.y >= 0.0 && p.z >= 0.0);
                }
            }
        }
    }
}
