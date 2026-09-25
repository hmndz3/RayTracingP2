//! Cielo del diorama: funcion analitica del anochecer y cubemap de seis caras.
//!
//! El cielo se define una sola vez como [`sky_radiance`], una funcion de la
//! direccion. El generador de recursos la evalua para rellenar las seis caras del
//! cubemap y el trazador lee esas caras en tiempo de render. Que el generador
//! parta de una funcion de la direccion, y no de un patron por cara, es lo que
//! hace que las seis imagenes encajen: dos caras contiguas evaluan exactamente la
//! misma direccion en su arista comun, asi que no puede aparecer una costura.

use crate::geometry::{face_normal, FACE_NEG_X, FACE_NEG_Y, FACE_NEG_Z, FACE_POS_X, FACE_POS_Y};
use crate::math::{smoothstep, v3, Vec3};
use crate::noise::directional;
use crate::texture::Texture;

/// Direccion hacia la luz principal, el resto de sol bajo del atardecer.
///
/// Azimut de unos -70 grados y elevacion de 14. La eleccion no es arbitraria: con
/// la vista inicial son visibles las caras `-X`, `+Y` y `+Z`, y esta direccion
/// ilumina de frente las `-X` mientras roza las `+Y` y las `+Z`. Ese rasado es lo
/// que hace legible el relieve de los mapas normales sobre la piedra.
pub const SUN_DIR: Vec3 = v3(-0.9106, 0.2419, 0.3321);

/// Direccion hacia la luna, que da el contrapunto frio al ambar del poniente.
pub const MOON_DIR: Vec3 = v3(0.3827, 0.5736, -0.7244);

/// Semilla del cielo. Se fija aparte de la del terreno para que cambiar el relieve
/// no cambie tambien las estrellas.
pub const SKY_SEED: u64 = 0x5AFE_C0DE_1234;

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

    // Bandas de nube tenues: rompen la planitud sin dibujar formas reconocibles.
    let banda = directional(d, 2.4, SKY_SEED ^ 0x11, 4);
    let mascara_banda = smoothstep((banda - 0.42) / 0.45) * (1.0 - smoothstep(arriba / 0.6));
    color = color.lerp(v3(0.1750, 0.1450, 0.2450), mascara_banda * 0.55);

    // Resto de luz del poniente: un nucleo estrecho y un halo ancho, los dos
    // pegados al horizonte mediante una caida exponencial en altura.
    let hacia_sol = d.dot(SUN_DIR).max(0.0);
    let pegado = (-arriba * 3.6).exp();
    let nucleo = hacia_sol.powi(24) * 0.95;
    let halo = hacia_sol.powf(3.2) * 0.30 * pegado;
    let ancho = hacia_sol.powf(1.3) * 0.085 * pegado;
    color += v3(1.00, 0.545, 0.225) * (nucleo + halo + ancho);

    // Luna: disco pequeno de borde suave mas su propio halo frio.
    let hacia_luna = d.dot(MOON_DIR).max(0.0);
    let disco = smoothstep((hacia_luna - 0.9988) / 0.0009);
    let halo_luna = hacia_luna.powf(180.0) * 0.22 + hacia_luna.powf(14.0) * 0.035;
    color += v3(0.86, 0.90, 1.00) * (disco * 0.85 + halo_luna);

    // Estrellas en dos capas. Se apagan cerca del horizonte y dentro del brillo
    // del poniente, que es donde el cielo real ya no las deja ver.
    let visibilidad = smoothstep(arriba / 0.16) * (1.0 - smoothstep(hacia_sol.powf(2.5) / 0.55));
    if visibilidad > 0.001 {
        let debiles = directional(d, 38.0, SKY_SEED ^ 0x22, 1);
        let brillantes = directional(d, 61.0, SKY_SEED ^ 0x33, 1);
        let f1 = smoothstep((debiles - 0.955) / 0.045);
        let f2 = smoothstep((brillantes - 0.980) / 0.020);
        let intensidad = f1 * f1 * 0.22 + f2 * f2 * f2 * 0.75;
        color += v3(0.92, 0.94, 1.00) * (intensidad * visibilidad);
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
pub fn render_face(face: usize, size: usize) -> crate::image::Image {
    use crate::texture::linear_to_srgb;
    let mut img = crate::image::Image::new(size, size);
    for y in 0..size {
        let v = (y as f64 + 0.5) / size as f64;
        for x in 0..size {
            let u = (x as f64 + 0.5) / size as f64;
            let c = sky_radiance(face_uv_to_direction(face, u, v));
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
                assert!(dif < 1e-12, "escala {escala} cambia el cielo en {dif}");
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
        assert!(media < 0.004, "costura media demasiado marcada: {media}");
        // El maximo admite las estrellas, que son puntos de un texel y caen a un
        // lado o a otro de la arista segun la cara.
        assert!(peor < 0.12, "salto puntual excesivo en una arista: {peor}");
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
