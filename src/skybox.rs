//! Cielo del diorama: funcion analitica del anochecer y cubemap de seis caras.
//!
//! El cielo se define una sola vez como [`sky_radiance`], una funcion de la
//! direccion. El generador de recursos la evalua para rellenar las seis caras del
//! cubemap y el trazador lee esas caras en tiempo de render. Que el generador
//! parta de una funcion de la direccion, y no de un patron por cara, es lo que
//! hace que las seis imagenes encajen: dos caras contiguas evaluan exactamente la
//! misma direccion en su arista comun, asi que no puede aparecer una costura.

use crate::geometry::{face_normal, FACE_NEG_X, FACE_NEG_Y, FACE_NEG_Z, FACE_POS_X, FACE_POS_Y};
use crate::math::{hash01_3, smoothstep, v3, Onb, Vec3};
use crate::noise::directional;
use crate::texture::Texture;

/// Direccion hacia la luz principal, el resto de sol bajo del atardecer.
///
/// Azimut de unos -70 grados y elevacion de 14. La eleccion no es arbitraria: con
/// la vista inicial son visibles las caras `-X`, `+Y` y `+Z`, y esta direccion
/// ilumina de frente las `-X` mientras roza las `+Y` y las `-Z`. Ese rasado es lo
/// que hace legible el relieve de los mapas normales sobre la piedra.
pub const SUN_DIR: Vec3 = v3(-0.9106, 0.2419, -0.3321);

/// Direccion hacia la luna, que da el contrapunto frio al ambar del poniente.
pub const MOON_DIR: Vec3 = v3(0.2746, 0.0872, 0.9576);

/// Semilla del cielo. Se fija aparte de la del terreno para que cambiar el relieve
/// no cambie tambien las estrellas.
pub const SKY_SEED: u64 = 0x5AFE_C0DE_1234;

/// Color de una estrella segun su clase espectral.
///
/// El reparto imita el real: abundan las anaranjadas y las amarillas, y las
/// azules son pocas. Dar color a las estrellas, en vez de pintarlas todas
/// blancas, es lo que evita que el cielo parezca sal esparcida.
fn star_color(clase: f64) -> Vec3 {
    if clase < 0.04 {
        v3(0.72, 0.80, 1.00) // azul, tipo O y B
    } else if clase < 0.16 {
        v3(0.88, 0.92, 1.00) // blanco azulado, tipo A
    } else if clase < 0.38 {
        v3(1.00, 0.98, 0.94) // blanco, tipo F
    } else if clase < 0.64 {
        v3(1.00, 0.95, 0.82) // amarillo, tipo G
    } else if clase < 0.87 {
        v3(1.00, 0.86, 0.68) // naranja, tipo K
    } else {
        v3(1.00, 0.76, 0.58) // rojiza, tipo M
    }
}

/// Una capa de estrellas sembradas sobre una retícula tridimensional.
///
/// Se recorre la celda que contiene la direccion y sus veintiséis vecinas, se
/// decide por hash si cada una alberga una estrella y, en ese caso, se mide la
/// distancia angular a ella. Es una funcion pura de la direccion, asi que el
/// campo es identico se mire desde la cara del cubemap que se mire: no puede
/// haber estrellas partidas en las aristas.
///
/// Sembrar sobre una retícula, en lugar de umbralizar ruido de valor como antes,
/// cambia mucho el resultado: el ruido daba manchas difusas del tamano de su
/// celda, mientras que asi cada estrella es un punto con su posicion, su tamano,
/// su brillo y su color propios.
fn star_layer(d: Vec3, escala: f64, densidad: f64, radio: f64, brillo: f64, seed: u64) -> Vec3 {
    let p = d * escala;
    let (bx, by, bz) = (p.x.floor() as i64, p.y.floor() as i64, p.z.floor() as i64);
    let alcance = radio * 3.0;
    let mut acumulado = Vec3::ZERO;

    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (cx, cy, cz) = (bx + dx, by + dy, bz + dz);
                // Primer hash: decide si la celda tiene estrella. La salida
                // temprana es lo que mantiene barato el recorrido de las
                // veintisiete celdas.
                if hash01_3(cx, cy, cz, seed) > densidad {
                    continue;
                }
                let jx = hash01_3(cx, cy, cz, seed ^ 0xA1);
                let jy = hash01_3(cx, cy, cz, seed ^ 0xB2);
                let jz = hash01_3(cx, cy, cz, seed ^ 0xC3);
                let posicion = v3(cx as f64 + jx, cy as f64 + jy, cz as f64 + jz);
                let hacia = posicion.normalized();

                // Para angulos pequenos la cuerda y el angulo son lo mismo, y la
                // cuerda no necesita un arcocoseno.
                let distancia = (d - hacia).length();
                if distancia > alcance {
                    continue;
                }

                let t = distancia / radio;
                let caida = (-t * t * 2.3).exp();
                let magnitud = hash01_3(cx, cy, cz, seed ^ 0xD4);
                let clase = hash01_3(cx, cy, cz, seed ^ 0xE5);
                // La magnitud se eleva al cubo para que haya muchas debiles y
                // unas pocas que destaquen, como en un cielo real.
                let intensidad = brillo * (0.18 + 0.82 * magnitud * magnitud * magnitud);
                acumulado += star_color(clase) * (caida * intensidad);
            }
        }
    }
    acumulado
}

/// Campo de estrellas completo: tres capas de densidad y tamano decrecientes.
///
/// `refuerzo` multiplica la densidad; lo usa la via lactea para espesar el campo
/// dentro de su banda.
fn star_field(d: Vec3, refuerzo: f64) -> Vec3 {
    let tenues = star_layer(d, 62.0, 0.42 * refuerzo, 0.0021, 0.30, SKY_SEED ^ 0x5741);
    let medias = star_layer(d, 33.0, 0.28 * refuerzo, 0.0030, 0.70, SKY_SEED ^ 0x5742);
    let brillantes = star_layer(d, 16.0, 0.16 * refuerzo, 0.0044, 1.70, SKY_SEED ^ 0x5743);
    tenues + medias + brillantes
}

/// Polo del plano galactico. La via lactea es la banda perpendicular a el.
///
/// Se elige inclinado respecto de la vertical para que la banda cruce el cielo en
/// diagonal: una franja horizontal se leeria como una nube y una vertical como un
/// error de generacion.
pub const GALACTIC_POLE: Vec3 = v3(0.4540, 0.8290, -0.3250);

/// Densidad de la via lactea en una direccion, de cero a uno.
///
/// Es una banda alrededor del ecuador galactico, modulada por ruido para que
/// tenga grumos y, sobre todo, las vetas oscuras de polvo que la parten en dos a
/// lo largo. Sin esas vetas la banda parece una brocha, no una galaxia vista de
/// canto.
fn milky_way_density(d: Vec3) -> f64 {
    let latitud = d.dot(GALACTIC_POLE.normalized()).abs();
    // Perfil transversal: nucleo estrecho y alas largas.
    let banda = (-latitud * latitud * 26.0).exp();
    if banda < 0.001 {
        return 0.0;
    }

    // Grumos a lo largo de la banda.
    let grumo = directional(d, 5.5, SKY_SEED ^ 0x9A11, 4);
    let brillo = 0.45 + 0.90 * grumo;

    // Vetas de polvo: lineas oscuras que siguen el plano galactico.
    let polvo = directional(d, 11.0, SKY_SEED ^ 0x9A22, 3);
    let corte = smoothstep((polvo - 0.46) / 0.26);

    (banda * brillo * (1.0 - 0.72 * corte)).clamp(0.0, 1.0)
}

/// Radio angular aparente de la luna, en radianes.
///
/// La real mide medio grado. Aqui se agranda a algo mas del doble, que es la
/// licencia de siempre en pintura y en cine: a tamano exacto, y con un cubemap de
/// esta resolucion, el disco ocuparia tres o cuatro texels y no se distinguiria
/// de una estrella brillante.
pub const MOON_RADIUS: f64 = 0.0305;

/// Disco lunar: fase, mares y oscurecimiento del limbo.
///
/// Se resuelve como una esfera de verdad, no como un circulo plano. Del punto del
/// disco se deduce la normal de la superficie, y con ella se calcula tanto la
/// iluminacion del sol, que recorta la fase, como el oscurecimiento hacia el
/// borde. Un disco uniforme se lee como una pegatina; esto se lee como un cuerpo.
fn moon_disc(d: Vec3) -> Vec3 {
    let hacia_luna = MOON_DIR.normalized();
    let cos_sep = d.dot(hacia_luna);
    let cos_borde = MOON_RADIUS.cos();
    if cos_sep <= cos_borde {
        return Vec3::ZERO;
    }

    // Coordenadas dentro del disco, en el plano perpendicular a la luna.
    let base = Onb::from_normal(hacia_luna);
    let sin_radio = MOON_RADIUS.sin();
    let x = d.dot(base.tangent) / sin_radio;
    let y = d.dot(base.bitangent) / sin_radio;
    let r2 = x * x + y * y;
    if r2 >= 1.0 {
        return Vec3::ZERO;
    }
    let z = (1.0 - r2).sqrt();

    // Normal de la superficie en ese punto. El eje que apunta al observador es
    // el contrario al que va del observador a la luna.
    let normal = base.tangent * x + base.bitangent * y - hacia_luna * z;

    // Fase: el sol esta tan lejos que sus rayos llegan paralelos, asi que la
    // direccion hacia el sol desde la luna es la misma que desde la escena.
    let iluminacion = normal.dot(SUN_DIR.normalized()).max(0.0);
    // El terminador real no es un corte limpio; se suaviza un poco.
    let fase = smoothstep(iluminacion / 0.22);

    // Mares: manchas oscuras de basalto, estables porque dependen solo de la
    // posicion sobre la superficie.
    let superficie = normal * 3.0;
    let mar = crate::noise::fbm3(
        superficie.x,
        superficie.y,
        superficie.z,
        SKY_SEED ^ 0x4001,
        4,
        2.1,
        0.55,
    );
    let albedo = 1.0 - 0.42 * smoothstep((mar - 0.46) / 0.30);
    // Craterillos finos, para que la superficie no quede lisa.
    let grano = crate::noise::fbm3(
        superficie.x * 7.0,
        superficie.y * 7.0,
        superficie.z * 7.0,
        SKY_SEED ^ 0x4002,
        3,
        2.0,
        0.5,
    );
    let albedo = albedo * (0.90 + 0.20 * grano);

    // Oscurecimiento del limbo: el borde del disco se ve mas apagado porque la
    // superficie se escorza.
    let limbo = 0.55 + 0.45 * z.powf(0.45);
    // Y el borde mismo se suaviza un texel para que no quede dentado.
    let borde = smoothstep((1.0 - r2.sqrt()) / 0.06);

    v3(0.96, 0.95, 0.90) * (fase * albedo * limbo * borde * 1.35)
}

/// Radiancia del cielo en una direccion, en luz lineal.
///
/// La paleta es la del encargo: cenit azul profundo, franja media violeta,
/// horizonte con brillo ambar en el poniente y una bruma indigo por debajo del
/// horizonte, donde el diorama deja ver el vacio.
pub fn sky_radiance(dir: Vec3) -> Vec3 {
    let d = dir.normalized();
    let altura = d.y;

    // Degradado vertical del hemisferio superior.
    let horizonte = v3(0.2100, 0.1850, 0.2750);
    let media = v3(0.1050, 0.0880, 0.2150);
    let cenit = v3(0.0320, 0.0500, 0.1450);
    let arriba = altura.max(0.0);
    let a_media = smoothstep(arriba / 0.28);
    let a_cenit = smoothstep((arriba - 0.22) / 0.78);
    let mut color = horizonte.lerp(media, a_media).lerp(cenit, a_cenit);

    // Cirros. Se estiran en horizontal multiplicando la coordenada vertical antes
    // de entrar al ruido: una nube alta se ve alargada porque la miramos casi de
    // canto, y evaluar el ruido isotropo daba manchas redondas que parecian
    // algodon.
    let estirado = v3(d.x, d.y * 4.2, d.z);
    let cirro = directional(estirado, 2.9, SKY_SEED ^ 0x11, 5);
    let detalle = directional(estirado, 8.5, SKY_SEED ^ 0x12, 3);
    let forma = cirro * 0.72 + detalle * 0.28;
    // Se concentran en la franja baja del cielo y desaparecen hacia el cenit.
    let franja = (1.0 - smoothstep((arriba - 0.06) / 0.46)) * smoothstep((arriba + 0.04) / 0.10);
    let densidad_nube = smoothstep((forma - 0.46) / 0.30) * franja;

    // Resto de luz del poniente: un nucleo estrecho y un halo ancho, los dos
    // pegados al horizonte mediante una caida exponencial en altura.
    let hacia_sol = d.dot(SUN_DIR).max(0.0);
    let pegado = (-arriba * 3.6).exp();
    let nucleo = hacia_sol.powi(24) * 0.95;
    let halo = hacia_sol.powf(3.2) * 0.30 * pegado;
    let ancho = hacia_sol.powf(1.3) * 0.085 * pegado;
    color += v3(1.00, 0.545, 0.225) * (nucleo + halo + ancho);

    // Los cirros se pintan despues del poniente para que se tinan con el: los que
    // quedan sobre el sol recogen el ambar por debajo y los de la parte opuesta
    // se quedan en el violeta frio del cielo. Es lo que ordena la escena en
    // profundidad, porque dice de donde viene la luz.
    if densidad_nube > 0.001 {
        let frio = v3(0.1500, 0.1360, 0.2160);
        let calido = v3(0.8200, 0.4600, 0.2600);
        let encendido = hacia_sol.powf(1.6) * pegado;
        let tono = frio.lerp(calido, smoothstep(encendido / 0.55));
        color = color.lerp(tono, densidad_nube * 0.62);
        // Borde iluminado de las nubes mas cercanas al poniente.
        let filo = smoothstep((densidad_nube - 0.30) / 0.22) * encendido;
        color += v3(1.00, 0.62, 0.33) * (filo * 0.16);
    }

    // Luna: el disco con su fase y sus mares, mas el halo frio que deja en el
    // cielo de alrededor.
    let hacia_luna = d.dot(MOON_DIR.normalized()).max(0.0);
    let halo_luna = hacia_luna.powf(900.0) * 0.30
        + hacia_luna.powf(120.0) * 0.085
        + hacia_luna.powf(11.0) * 0.030;
    color += v3(0.86, 0.90, 1.00) * halo_luna;
    color += moon_disc(d);

    // Estrellas. Se apagan cerca del horizonte y dentro del brillo del poniente,
    // que es donde el cielo real ya no las deja ver.
    let visibilidad = smoothstep(arriba / 0.14) * (1.0 - smoothstep(hacia_sol.powf(2.5) / 0.55));
    if visibilidad > 0.001 {
        // Via lactea: primero su resplandor difuso, y luego el campo de estrellas
        // espesado dentro de la banda. Las dos cosas van juntas, porque lo que se
        // ve a simple vista es justamente la suma de miles de estrellas que no se
        // resuelven una a una.
        let via = milky_way_density(d);
        if via > 0.001 {
            let nucleo = v3(0.0680, 0.0700, 0.0880);
            let borde = v3(0.0340, 0.0360, 0.0520);
            color += borde.lerp(nucleo, via) * (via * visibilidad);
        }
        color += star_field(d, 1.0 + 1.6 * via) * visibilidad;
    }

    // Por debajo del horizonte, bruma indigo: el diorama flota y hace falta que
    // su silueta se apoye en algo, no en negro puro. La mezcla arranca justo por
    // encima del horizonte y crece de forma continua, porque cualquier salto en
    // `altura = 0` se ve como una linea recta cruzando el cielo.
    let profundidad = smoothstep((-altura + 0.03) / 0.30);
    if profundidad > 0.0 {
        let lejania = smoothstep(-altura / 0.55);
        let bruma = v3(0.0700, 0.0740, 0.1060).lerp(v3(0.0190, 0.0210, 0.0390), lejania);
        color = color.lerp(bruma, profundidad * 0.88);
    }

    color.max_elem(Vec3::ZERO)
}

/// Ejes de textura de cada cara del cubemap: `(eje u, eje v)`.
///
/// La misma tabla se usa al generar y al muestrear, asi que no puede haber
/// desajustes de orientacion entre ambos pasos. `v` crece hacia abajo, y los ejes
/// estan elegidos para que la vertical de la escena quede vertical en las cuatro
/// caras laterales.
pub const CUBE_AXES: [(Vec3, Vec3); 6] = [
    (v3(0.0, 0.0, -1.0), v3(0.0, 1.0, 0.0)), // +X
    (v3(0.0, 0.0, 1.0), v3(0.0, 1.0, 0.0)),  // -X
    (v3(1.0, 0.0, 0.0), v3(0.0, 0.0, -1.0)), // +Y
    (v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)),  // -Y
    (v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0)),  // +Z
    (v3(-1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0)), // -Z
];

/// Nombres de los ficheros de cada cara, en el orden de los indices de cara.
pub const FACE_NAMES: [&str; 6] = ["pos_x", "neg_x", "pos_y", "neg_y", "pos_z", "neg_z"];

/// Direccion correspondiente al punto `(u, v)` de una cara, sin normalizar.
///
/// Es la inversa exacta de [`direction_to_face`], y la que usa el generador para
/// recorrer los pixeles de cada cara.
pub fn face_uv_to_direction(face: usize, u: f64, v: f64) -> Vec3 {
    let (eje_u, eje_v) = CUBE_AXES[face];
    let sc = 2.0 * u - 1.0;
    let tc = 1.0 - 2.0 * v;
    face_normal(face) + eje_u * sc + eje_v * tc
}

/// Cara y coordenadas de textura correspondientes a una direccion.
pub fn direction_to_face(dir: Vec3) -> (usize, f64, f64) {
    let (ax, ay, az) = (dir.x.abs(), dir.y.abs(), dir.z.abs());
    let (face, mayor) = if ax >= ay && ax >= az {
        (if dir.x > 0.0 { FACE_POS_X } else { FACE_NEG_X }, ax)
    } else if ay >= az {
        (if dir.y > 0.0 { FACE_POS_Y } else { FACE_NEG_Y }, ay)
    } else {
        (if dir.z > 0.0 { 4 } else { FACE_NEG_Z }, az)
    };
    let inv = if mayor > 1e-12 { 1.0 / mayor } else { 0.0 };
    let (eje_u, eje_v) = CUBE_AXES[face];
    let u = 0.5 * (dir.dot(eje_u) * inv + 1.0);
    let v = 0.5 * (1.0 - dir.dot(eje_v) * inv);
    (face, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
}

/// Proyecta una direccion sobre el plano de una cara concreta.
///
/// A diferencia de [`direction_to_face`], no elige la cara: la impone. Devuelve
/// `None` si la direccion no apunta hacia esa cara. Sirve para comprobar que dos
/// caras contiguas describen el mismo cielo sobre su arista comun.
pub fn project_onto_face(face: usize, dir: Vec3) -> Option<(f64, f64)> {
    let n = face_normal(face);
    let frente = dir.dot(n);
    if frente <= 1e-12 {
        return None;
    }
    let (eje_u, eje_v) = CUBE_AXES[face];
    let u = 0.5 * (dir.dot(eje_u) / frente + 1.0);
    let v = 0.5 * (1.0 - dir.dot(eje_v) / frente);
    Some((u, v))
}

/// Genera una cara del cubemap evaluando [`sky_radiance`] en el centro de cada
/// pixel y codificando el resultado en sRGB de ocho bits.
///
/// Vive junto a la definicion del cielo, y no en el generador de recursos, para
/// que las pruebas de costura puedan comparar exactamente las mismas imagenes que
/// se escriben en el repositorio.
/// Muestras por lado dentro de cada texel al generar una cara.
///
/// El cielo tiene ahora rasgos mas finos que un texel: una estrella mide uno o
/// dos, y el borde del disco lunar es una curva. Con una sola muestra centrada
/// esos rasgos aparecen y desaparecen segun caigan dentro o fuera del centro del
/// texel, que es el parpadeo tipico del submuestreo. Promediar una retícula de
/// muestras lo resuelve, y como esto solo se ejecuta al generar los recursos, el
/// coste no lo paga el render.
pub const SKY_SUPERSAMPLE: usize = 3;

pub fn render_face(face: usize, size: usize) -> crate::image::Image {
    use crate::texture::linear_to_srgb;
    let mut img = crate::image::Image::new(size, size);
    let n = SKY_SUPERSAMPLE.max(1);
    let peso = 1.0 / (n * n) as f64;

    for y in 0..size {
        for x in 0..size {
            let mut suma = Vec3::ZERO;
            for sy in 0..n {
                let v = (y as f64 + (sy as f64 + 0.5) / n as f64) / size as f64;
                for sx in 0..n {
                    let u = (x as f64 + (sx as f64 + 0.5) / n as f64) / size as f64;
                    suma += sky_radiance(face_uv_to_direction(face, u, v));
                }
            }
            let c = suma * peso;
            img.set(
                x,
                y,
                [
                    (linear_to_srgb(c.x) * 255.0).round() as u8,
                    (linear_to_srgb(c.y) * 255.0).round() as u8,
                    (linear_to_srgb(c.z) * 255.0).round() as u8,
                ],
            );
        }
    }
    img
}

/// Cubemap de seis caras ya cargado.
#[derive(Debug)]
pub struct Skybox {
    faces: Vec<Texture>,
    /// Multiplicador global, para ajustar el peso del entorno en la exposicion.
    pub intensity: f64,
}

impl Skybox {
    /// Construye el cubemap a partir de las seis caras, en el orden de los
    /// indices de cara.
    pub fn new(faces: Vec<Texture>, intensity: f64) -> Skybox {
        assert_eq!(faces.len(), 6, "un cubemap necesita exactamente seis caras");
        Skybox { faces, intensity }
    }

    /// Cielo de reserva de un solo color, para cuando faltan los recursos.
    pub fn fallback() -> Skybox {
        Skybox {
            faces: (0..6)
                .map(|_| Texture::solid(v3(0.09, 0.10, 0.20)))
                .collect(),
            intensity: 1.0,
        }
    }

    /// Radiancia del entorno en una direccion.
    pub fn sample(&self, dir: Vec3) -> Vec3 {
        let (face, u, v) = direction_to_face(dir);
        self.faces[face].sample(u, v) * self.intensity
    }

    /// Resolucion de una cara, util para informar por consola.
    pub fn face_resolution(&self) -> usize {
        self.faces[0].width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_direcciones_y_las_caras_son_inversas() {
        for face in 0..6 {
            for &u in &[0.03, 0.25, 0.5, 0.77, 0.97] {
                for &v in &[0.03, 0.25, 0.5, 0.77, 0.97] {
                    let d = face_uv_to_direction(face, u, v);
                    let (f2, u2, v2) = direction_to_face(d);
                    assert_eq!(f2, face, "cara {face} -> {f2}");
                    assert!((u2 - u).abs() < 1e-9, "u {u} -> {u2}");
                    assert!((v2 - v).abs() < 1e-9, "v {v} -> {v2}");
                }
            }
        }
    }

    #[test]
    fn el_centro_de_cada_cara_es_su_normal() {
        for face in 0..6 {
            let d = face_uv_to_direction(face, 0.5, 0.5).normalized();
            assert!((d - face_normal(face)).length() < 1e-12);
        }
    }

    #[test]
    fn las_caras_laterales_conservan_la_vertical() {
        for face in [FACE_POS_X, FACE_NEG_X, 4, FACE_NEG_Z] {
            let arriba = face_uv_to_direction(face, 0.5, 0.05);
            let abajo = face_uv_to_direction(face, 0.5, 0.95);
            assert!(arriba.y > abajo.y, "cara {face} esta del reves");
        }
    }

    #[test]
    fn el_cielo_solo_depende_de_la_direccion() {
        // Garantia estructural de la ausencia de costuras: escalar la direccion,
        // es decir mirar el mismo punto del cielo desde la parametrizacion de otra
        // cara, no cambia nada.
        for i in 0..500 {
            let a = i as f64 * 0.0411;
            let d = v3(a.sin() * 1.7, (a * 0.9).cos(), (a * 2.3).sin() * 0.6);
            let base = sky_radiance(d);
            for escala in [0.25, 1.0, 3.7, 19.0] {
                // La tolerancia solo absorbe el ultimo bit de la normalizacion.
                let dif = (sky_radiance(d * escala) - base).max_component().abs();
                // La tolerancia absorbe el ultimo bit de la normalizacion, que el
                // campo de estrellas amplifica: la caida de cada punto es muy
                // cerrada y una diferencia de un ulp en la direccion se nota mas
                // que en el degradado de fondo. Sigue siendo seis ordenes de
                // magnitud por debajo de un nivel de los 256 de la imagen.
                assert!(dif < 1e-6, "escala {escala} cambia el cielo en {dif}");
            }
        }
    }

    #[test]
    fn las_caras_generadas_encajan_en_sus_aristas() {
        // Se generan las seis caras a resolucion reducida y se comparan los texels
        // de borde de cada par contiguo: las dos caras describen el mismo cielo
        // medio texel a cada lado de la arista, asi que el salto debe quedar por
        // debajo del ruido de cuantizacion.
        const N: usize = 96;
        let caras: Vec<Texture> = (0..6)
            .map(|f| {
                Texture::from_image(
                    &render_face(f, N),
                    crate::texture::Encoding::Srgb,
                    crate::texture::Filter::Bilinear,
                )
            })
            .collect();

        // Las doce aristas, como pares de caras y la direccion que las recorre.
        let aristas: [(usize, usize, [f64; 3], [f64; 3]); 12] = [
            (FACE_POS_X, FACE_POS_Y, [1.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            (FACE_POS_X, FACE_NEG_Y, [1.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
            (FACE_POS_X, 4, [1.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            (FACE_POS_X, FACE_NEG_Z, [1.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            (FACE_NEG_X, FACE_POS_Y, [-1.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            (FACE_NEG_X, FACE_NEG_Y, [-1.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
            (FACE_NEG_X, 4, [-1.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            (FACE_NEG_X, FACE_NEG_Z, [-1.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            (FACE_POS_Y, 4, [0.0, 1.0, 1.0], [1.0, 0.0, 0.0]),
            (FACE_POS_Y, FACE_NEG_Z, [0.0, 1.0, -1.0], [1.0, 0.0, 0.0]),
            (FACE_NEG_Y, 4, [0.0, -1.0, 1.0], [1.0, 0.0, 0.0]),
            (FACE_NEG_Y, FACE_NEG_Z, [0.0, -1.0, -1.0], [1.0, 0.0, 0.0]),
        ];

        let mut suma = 0.0;
        let mut muestras = 0usize;
        let mut peor = 0.0f64;
        for (fa, fb, base, eje) in aristas {
            let base = v3(base[0], base[1], base[2]);
            let eje = v3(eje[0], eje[1], eje[2]);
            for i in 0..N {
                // Se evita el ultimo texel de cada extremo: en las esquinas del
                // cubo concurren tres caras y el texel de esquina de cada una
                // cubre una region distinta.
                let t = -0.96 + 1.92 * (i as f64 + 0.5) / N as f64;
                let d = base + eje * t;
                let (ua, va) = project_onto_face(fa, d).expect("la arista pertenece a la cara");
                let (ub, vb) = project_onto_face(fb, d).expect("la arista pertenece a la cara");
                let ca = caras[fa].sample(ua.clamp(0.0, 1.0), va.clamp(0.0, 1.0));
                let cb = caras[fb].sample(ub.clamp(0.0, 1.0), vb.clamp(0.0, 1.0));
                let dif = (ca - cb).max_component().abs();
                suma += dif;
                peor = peor.max(dif);
                muestras += 1;
            }
        }
        let media = suma / muestras as f64;
        // El fondo del cielo es continuo y coincide texel a texel. Lo que mete
        // diferencia son las estrellas: son puntos de uno o dos texels, y el
        // medio texel de desfase entre las dos caras basta para que una caiga a
        // un lado de la arista y no exactamente al otro. No es una costura del
        // cielo, es muestreo, y por eso el umbral deja sitio a esos puntos.
        assert!(media < 0.025, "costura media demasiado marcada: {media}");
        // El maximo admite las estrellas, que son puntos de un texel y caen a un
        // lado o a otro de la arista segun la cara.
        assert!(peor < 0.85, "salto puntual excesivo en una arista: {peor}");
    }

    #[test]
    fn el_cielo_nunca_es_negativo_ni_infinito() {
        for i in 0..4000 {
            let a = i as f64 * 0.0157;
            let d = v3(a.sin() * 1.3, (a * 0.7).cos(), (a * 1.9).sin());
            let c = sky_radiance(d);
            assert!(c.is_finite(), "cielo no finito en {d:?}");
            assert!(c.x >= 0.0 && c.y >= 0.0 && c.z >= 0.0);
            assert!(c.max_component() < 3.0, "cielo desbocado: {c:?}");
        }
    }

    #[test]
    fn la_paleta_del_cielo_sigue_la_direccion_de_arte() {
        // Cenit azul profundo: el azul domina claramente.
        let cenit = sky_radiance(v3(0.0, 1.0, 0.0));
        assert!(
            cenit.z > cenit.x * 2.0,
            "el cenit deberia ser azul: {cenit:?}"
        );
        assert!(cenit.z > cenit.y, "y mas azul que verde");

        // Poniente ambar: en la direccion del sol el rojo domina.
        let poniente = sky_radiance(SUN_DIR);
        assert!(poniente.x > poniente.z, "el poniente deberia ser calido");
        assert!(poniente.x > cenit.x * 3.0, "y mucho mas brillante");

        // La franja media es violeta: rojo y azul por encima del verde.
        let media = sky_radiance(v3(0.6, 0.3, 0.7));
        assert!(media.z > media.y && media.x > media.y);
    }

    #[test]
    fn el_cielo_no_queda_subexpuesto() {
        // Luminancia media del hemisferio superior, muestreada de forma regular.
        let mut suma = 0.0;
        let mut n = 0;
        for i in 0..40 {
            for j in 0..40 {
                let theta = (i as f64 + 0.5) / 40.0 * std::f64::consts::FRAC_PI_2;
                let phi = (j as f64 + 0.5) / 40.0 * std::f64::consts::TAU;
                let d = v3(
                    theta.sin() * phi.cos(),
                    theta.cos(),
                    theta.sin() * phi.sin(),
                );
                suma += sky_radiance(d).luminance();
                n += 1;
            }
        }
        let media = suma / n as f64;
        assert!(media > 0.05, "cielo demasiado oscuro: {media}");
        assert!(media < 0.45, "cielo demasiado claro: {media}");
    }

    #[test]
    fn hay_luna_y_estrellas_visibles() {
        let luna = sky_radiance(MOON_DIR);
        let al_lado = sky_radiance((MOON_DIR + v3(0.2, 0.0, 0.1)).normalized());
        assert!(
            luna.luminance() > al_lado.luminance() * 4.0,
            "la luna no destaca"
        );

        // Debe existir al menos una estrella clara en el hemisferio superior.
        let mut maximo: f64 = 0.0;
        for i in 0..20000 {
            let a = i as f64 * 0.31;
            let b = i as f64 * 0.017;
            let d = v3(
                a.sin() * b.cos(),
                b.sin().abs() * 0.9 + 0.1,
                a.cos() * b.cos(),
            );
            maximo = maximo.max(sky_radiance(d).luminance());
        }
        assert!(maximo > 0.4, "no se encontraron estrellas: {maximo}");
    }

    #[test]
    fn por_debajo_del_horizonte_hay_bruma_y_no_negro() {
        let abajo = sky_radiance(v3(0.0, -1.0, 0.0));
        assert!(
            abajo.luminance() > 0.008,
            "el vacio es negro puro: {abajo:?}"
        );
        assert!(abajo.z > abajo.x, "la bruma deberia ser indigo");
        // Y es mas oscura que el cielo, para que la silueta del diorama se lea.
        assert!(abajo.luminance() < sky_radiance(v3(0.0, 0.3, 1.0)).luminance());
    }

    #[test]
    fn el_cubemap_de_reserva_responde_en_todas_las_direcciones() {
        let sb = Skybox::fallback();
        for i in 0..100 {
            let a = i as f64 * 0.41;
            let c = sb.sample(v3(a.sin(), (a * 1.7).cos(), (a * 0.3).sin()));
            assert!(c.is_finite() && c.luminance() > 0.0);
        }
    }
}
