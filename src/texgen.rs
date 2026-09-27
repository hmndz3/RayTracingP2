//! Generador de los recursos graficos del proyecto.
//!
//! Escribe las texturas de albedo, los mapas normales y las seis caras del
//! cubemap como PPM dentro de `assets/`. Los ficheros resultantes se guardan en el
//! repositorio, de modo que el render no genera nada en tiempo de ejecucion ni
//! descarga nada: el trazador solo lee.
//!
//! Los colores se escriben como reflectancia lineal, que es la magnitud que
//! consume la iluminacion, y se codifican a sRGB al guardar. Las texturas son de
//! 32 pixeles de lado, con lo que un bloque del diorama mide 32 texels y el
//! aspecto pixelado queda consistente en toda la escena.

use crate::image::Image;
use crate::math::{smoothstep, v3, Vec3};
use crate::noise::{cell2, fbm2, ridged2, value2};
use crate::skybox::{render_face, FACE_NAMES};
use crate::texture::linear_to_srgb;
use std::path::Path;

/// Lado en pixeles de las texturas de material.
pub const TEX_SIZE: usize = 32;
/// Lado en pixeles del vitral, que necesita mas detalle para su celosia.
pub const VITRAL_SIZE: usize = 64;
/// Lado en pixeles de cada cara del cubemap.
pub const SKY_FACE_SIZE: usize = 256;

/// Construye una textura evaluando una funcion de color lineal por pixel.
fn build(size: usize, f: impl Fn(f64, f64) -> Vec3) -> Image {
    let mut img = Image::new(size, size);
    for y in 0..size {
        let v = (y as f64 + 0.5) / size as f64;
        for x in 0..size {
            let u = (x as f64 + 0.5) / size as f64;
            let c = f(u, v);
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

/// Construye un mapa normal a partir de un campo de altura.
///
/// Las derivadas se toman por diferencias centrales con indices circulares, asi
/// que el mapa es continuo al repetirse de un bloque al siguiente. El signo de las
/// dos componentes es el mismo porque la bitangente de cada cara es la derivada de
/// la posicion respecto de `v`, y `v` crece hacia abajo: una altura que aumenta
/// hacia abajo inclina la normal hacia arriba de la imagen.
fn normal_map(size: usize, strength: f64, height: impl Fn(usize, usize) -> f64) -> Image {
    let mut alturas = vec![0.0f64; size * size];
    for y in 0..size {
        for x in 0..size {
            alturas[y * size + x] = height(x, y);
        }
    }
    let at = |x: isize, y: isize| -> f64 {
        let xi = x.rem_euclid(size as isize) as usize;
        let yi = y.rem_euclid(size as isize) as usize;
        alturas[yi * size + xi]
    };

    let mut img = Image::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let (xi, yi) = (x as isize, y as isize);
            let dhdu = (at(xi + 1, yi) - at(xi - 1, yi)) * 0.5;
            let dhdv = (at(xi, yi + 1) - at(xi, yi - 1)) * 0.5;
            let n = v3(-dhdu * strength, -dhdv * strength, 1.0).normalized();
            img.set(
                x,
                y,
                [
                    ((n.x * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8,
                    ((n.y * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8,
                    ((n.z * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8,
                ],
            );
        }
    }
    img
}

/// Retícula de sillares a juntas alternas.
///
/// Devuelve `(identificador de sillar, u local, v local, distancia a la junta en
/// pixeles)`.
fn ashlar(u: f64, v: f64, size: f64, cols: f64, rows: f64) -> (f64, f64, f64, f64) {
    let fila = (v * rows).floor();
    let desplazamiento = if (fila as i64).rem_euclid(2) == 0 {
        0.0
    } else {
        0.5
    };
    let ux = u * cols + desplazamiento;
    let columna = ux.floor();
    let lu = ux - columna;
    let lv = v * rows - fila;
    let du = lu.min(1.0 - lu) * (size / cols);
    let dv = lv.min(1.0 - lv) * (size / rows);
    let id = crate::math::hash01_3(columna as i64, fila as i64, 0, 0x5111);
    (id, lu, lv, du.min(dv))
}

/// Piedra antigua de los muros: sillares azul grisaceos con junta de mortero.
fn stone_ancient_albedo(u: f64, v: f64) -> Vec3 {
    let (id, _lu, lv, junta) = ashlar(u, v, TEX_SIZE as f64, 2.0, 4.0);
    let mortero = v3(0.078, 0.088, 0.108);
    let base = v3(0.178, 0.204, 0.252);

    if junta < 1.0 {
        let grano = value2(u * 54.0, v * 54.0, 0x7711);
        return mortero * (0.82 + 0.30 * grano);
    }

    // Variacion por sillar, para que un muro largo no parezca una sola losa.
    let mut c = base * (0.84 + 0.30 * id);
    // Grano fino y algunas picaduras.
    let grano = fbm2(u * 13.0, v * 13.0, 0x1234, 3, 2.0, 0.55);
    c *= 0.86 + 0.28 * grano;
    let picadura = cell2(u * 11.0, v * 11.0, 0x99AA);
    if picadura.distance < 0.14 {
        c *= 0.70;
    }
    // Musgo acumulado en la parte baja de cada sillar y junto a las juntas.
    let humedad = smoothstep((lv - 0.45) / 0.55) * 0.8 + smoothstep((3.0 - junta) / 3.0) * 0.35;
    let mancha = fbm2(u * 9.0, v * 9.0, 0x4242, 4, 2.1, 0.5);
    let musgo = smoothstep((mancha * humedad - 0.30) / 0.35);
    c = c.lerp(v3(0.108, 0.158, 0.088), musgo * 0.75);
    // Ligera tincion calida donde la luz del poniente lleva anos dando.
    c.lerp(v3(0.216, 0.196, 0.164), 0.12 * smoothstep(id))
}

fn stone_ancient_height(x: usize, y: usize) -> f64 {
    let u = (x as f64 + 0.5) / TEX_SIZE as f64;
    let v = (y as f64 + 0.5) / TEX_SIZE as f64;
    let (id, _, _, junta) = ashlar(u, v, TEX_SIZE as f64, 2.0, 4.0);
    // El sillar sobresale sobre el mortero con un bisel de poco mas de un pixel.
    // No hay corte abrupto en la junta: un escalon de altura en un solo texel
    // haria que la diferencia central saturase la normal en toda la retícula de
    // juntas, y el mapa se leeria como grano en vez de como relieve.
    let bisel = smoothstep(junta / 1.15);
    let relieve = fbm2(u * 7.0, v * 7.0, 0x1234, 3, 2.0, 0.55);
    // La picadura se hunde de forma suave: un escalon duro produciria un
    // gradiente enorme y el mapa normal saldria saturado en ese texel.
    let picadura = cell2(u * 11.0, v * 11.0, 0x99AA);
    let hueco = smoothstep((0.14 - picadura.distance) / 0.14) * 0.14;
    (0.55 + 0.20 * id) * bisel + 0.11 * relieve - hueco
}

/// Losa del camino y del suelo interior: piedra irregular, mas calida.
fn stone_floor_albedo(u: f64, v: f64) -> Vec3 {
    let c = cell2(u * 3.4, v * 3.4, 0x2BAD);
    let junta = smoothstep(c.border / 0.10);
    let base = v3(0.168, 0.174, 0.192).lerp(v3(0.232, 0.220, 0.206), c.id);
    let grano = fbm2(u * 40.0, v * 40.0, 0x5150, 3, 2.0, 0.5);
    let mut color = base * (0.84 + 0.30 * grano);
    // Mortero de tierra entre losas.
    color = v3(0.104, 0.092, 0.076).lerp(color, junta);
    // Musgo colonizando las juntas.
    let musgo = smoothstep((fbm2(u * 7.0, v * 7.0, 0x7007, 4, 2.0, 0.5) - 0.42) / 0.30);
    color.lerp(v3(0.096, 0.146, 0.082), musgo * (1.0 - junta) * 0.85)
}

fn stone_floor_height(x: usize, y: usize) -> f64 {
    let u = (x as f64 + 0.5) / TEX_SIZE as f64;
    let v = (y as f64 + 0.5) / TEX_SIZE as f64;
    let c = cell2(u * 3.4, v * 3.4, 0x2BAD);
    let losa = smoothstep(c.border / 0.13);
    losa * (0.55 + 0.25 * c.id) + 0.14 * fbm2(u * 6.0, v * 6.0, 0x5150, 3, 2.0, 0.5)
}

/// Escombro y lapidas: piedra mas clara, rota y desgastada.
fn stone_rubble_albedo(u: f64, v: f64) -> Vec3 {
    let c = cell2(u * 5.2, v * 5.2, 0x3C3C);
    let base = v3(0.204, 0.208, 0.222).lerp(v3(0.262, 0.256, 0.246), c.id);
    let grano = fbm2(u * 52.0, v * 52.0, 0x6161, 4, 2.0, 0.5);
    let mut color = base * (0.80 + 0.36 * grano);
    // Aristas descantilladas: el borde de cada fragmento aclara.
    color = color.lerp(
        v3(0.452, 0.440, 0.418),
        (1.0 - smoothstep(c.border / 0.08)) * 0.5,
    );
    let musgo = smoothstep((fbm2(u * 8.0, v * 8.0, 0x1A1A, 4, 2.0, 0.5) - 0.50) / 0.28);
    color.lerp(v3(0.102, 0.150, 0.086), musgo * 0.55)
}

fn stone_rubble_height(x: usize, y: usize) -> f64 {
    let u = (x as f64 + 0.5) / TEX_SIZE as f64;
    let v = (y as f64 + 0.5) / TEX_SIZE as f64;
    let c = cell2(u * 5.2, v * 5.2, 0x3C3C);
    smoothstep(c.border / 0.10) * (0.5 + 0.3 * c.id)
        + 0.18 * fbm2(u * 7.0, v * 7.0, 0x6161, 4, 2.0, 0.5)
}

/// Madera envejecida de vigas, pasarela y portones.
fn wood_aged_albedo(u: f64, v: f64) -> Vec3 {
    // Tablas separadas en v, veta corriendo a lo largo de u.
    let tablas = 4.0;
    let fila = (v * tablas).floor();
    let lv = v * tablas - fila;
    let separacion = lv.min(1.0 - lv) * (TEX_SIZE as f64 / tablas);
    let id = crate::math::hash01_3(0, fila as i64, 0, 0xBEEF);

    if separacion < 0.9 {
        return v3(0.038, 0.026, 0.018);
    }

    // La veta se estira: mucha frecuencia en v y poca en u.
    let veta = ridged2(u * 3.0 + id * 7.0, v * 30.0, 0xD00D, 4);
    let oscura = v3(0.086, 0.054, 0.030);
    let clara = v3(0.208, 0.140, 0.082);
    let mut c = oscura.lerp(clara, veta.powf(0.8));
    c *= 0.88 + 0.24 * id;
    // Nudos ocasionales.
    let nudo = cell2(u * 3.0, v * 6.0, 0xC0FF);
    if nudo.distance < 0.11 {
        c = c.lerp(
            v3(0.052, 0.032, 0.020),
            smoothstep((0.11 - nudo.distance) / 0.11),
        );
    }
    // Grisado del sol y algo de musgo en las tablas mas bajas.
    c = c.lerp(
        v3(0.140, 0.126, 0.110),
        0.18 * fbm2(u * 12.0, v * 12.0, 0x3131, 3, 2.0, 0.5),
    );
    let musgo = smoothstep((fbm2(u * 6.0, v * 9.0, 0x5A5A, 3, 2.0, 0.5) - 0.58) / 0.25);
    c.lerp(v3(0.088, 0.128, 0.070), musgo * 0.45)
}

fn wood_aged_height(x: usize, y: usize) -> f64 {
    let u = (x as f64 + 0.5) / TEX_SIZE as f64;
    let v = (y as f64 + 0.5) / TEX_SIZE as f64;
    let tablas = 4.0;
    let fila = (v * tablas).floor();
    let lv = v * tablas - fila;
    let separacion = lv.min(1.0 - lv) * (TEX_SIZE as f64 / tablas);
    let id = crate::math::hash01_3(0, fila as i64, 0, 0xBEEF);
    let veta = ridged2(u * 3.0 + id * 7.0, v * 9.0, 0xD00D, 4);
    // La junta entre tablas baja de forma continua, por la misma razon que en la
    // piedra: un corte seco satura la normal en la linea de separacion.
    smoothstep(separacion / 1.3) * 0.62 + 0.16 * veta
}

/// Tierra con musgo: la capa superior del terreno.
fn earth_moss_albedo(u: f64, v: f64) -> Vec3 {
    let tierra_oscura = v3(0.070, 0.052, 0.034);
    let tierra_clara = v3(0.122, 0.090, 0.058);
    let grumo = fbm2(u * 18.0, v * 18.0, 0x1111, 4, 2.1, 0.55);
    let mut c = tierra_oscura.lerp(tierra_clara, grumo);

    // Musgo verde desaturado en manchas organicas.
    let mancha = fbm2(u * 5.5, v * 5.5, 0x2222, 4, 2.2, 0.55);
    let musgo = smoothstep((mancha - 0.40) / 0.32);
    let musgo_color = v3(0.082, 0.128, 0.064).lerp(v3(0.118, 0.170, 0.086), grumo);
    c = c.lerp(musgo_color, musgo * 0.92);

    // Guijarros dispersos.
    let piedra = cell2(u * 9.0, v * 9.0, 0x3333);
    if piedra.distance < 0.13 {
        c = c.lerp(
            v3(0.250, 0.252, 0.258),
            smoothstep((0.13 - piedra.distance) / 0.09) * 0.85,
        );
    }
    c * (0.88 + 0.22 * value2(u * 60.0, v * 60.0, 0x4444))
}

fn earth_moss_height(x: usize, y: usize) -> f64 {
    let u = (x as f64 + 0.5) / TEX_SIZE as f64;
    let v = (y as f64 + 0.5) / TEX_SIZE as f64;
    let grumo = fbm2(u * 6.0, v * 6.0, 0x1111, 4, 2.1, 0.55);
    let piedra = cell2(u * 9.0, v * 9.0, 0x3333);
    let bulto = if piedra.distance < 0.13 {
        smoothstep((0.13 - piedra.distance) / 0.09) * 0.7
    } else {
        0.0
    };
    grumo * 0.8 + bulto
}

/// Tierra profunda: lo que se ve en el corte del terreno y bajo el agua.
fn earth_dark_albedo(u: f64, v: f64) -> Vec3 {
    let base = v3(0.062, 0.046, 0.034);
    let vena = fbm2(u * 14.0, v * 22.0, 0x8888, 4, 2.0, 0.5);
    let mut c = base * (0.80 + 0.45 * vena);
    // Estratos horizontales tenues, que dan lectura de corte de terreno.
    let estrato = ((v * 6.0).sin() * 0.5 + 0.5).powf(3.0);
    c = c.lerp(v3(0.094, 0.076, 0.056), estrato * 0.35);
    let piedra = cell2(u * 7.0, v * 7.0, 0x9999);
    if piedra.distance < 0.10 {
        c = c.lerp(v3(0.150, 0.146, 0.140), 0.7);
    }
    c
}

/// Agua: tinte azul verdoso muy leve. El aspecto lo dan la refraccion y el
/// reflejo, no el color propio, asi que la textura apenas modula.
fn water_albedo(u: f64, v: f64) -> Vec3 {
    let n = fbm2(u * 6.0, v * 6.0, 0xAAA1, 3, 2.0, 0.5);
    v3(0.032, 0.084, 0.078).lerp(v3(0.046, 0.108, 0.096), n)
}

/// Altura de la onda del agua.
///
/// Es una composicion de senos de frecuencia entera, asi que se repite de forma
/// exacta de una celda a la siguiente: si se usara ruido sin periodo aparecerian
/// costuras en la retícula del estanque.
fn water_height(x: usize, y: usize) -> f64 {
    let u = (x as f64 + 0.5) / TEX_SIZE as f64;
    let v = (y as f64 + 0.5) / TEX_SIZE as f64;
    let tau = std::f64::consts::TAU;
    let a = (tau * (2.0 * u + 0.30 * (tau * v).sin())).sin();
    let b = (tau * (3.0 * v - 0.22 * (tau * 2.0 * u).sin())).sin();
    let c = (tau * (u + v)).sin();
    0.5 * a + 0.32 * b + 0.18 * c
}

/// Vitral: celosia de plomo con rosetón central y paneles de color.
///
/// Esta textura es la que tine la luz transmitida, asi que sus colores se eligen
/// saturados: al atravesar el vidrio multiplican el color del emisor que hay
/// detras.
fn stained_glass_albedo(u: f64, v: f64) -> Vec3 {
    let plomo = v3(0.030, 0.030, 0.034);
    let borde = (u.min(1.0 - u)).min(v.min(1.0 - v)) * VITRAL_SIZE as f64;
    if borde < 2.5 {
        return plomo;
    }

    let (cx, cy) = (u - 0.5, v - 0.5);
    let r = (cx * cx + cy * cy).sqrt();
    let angulo = cy.atan2(cx);

    let ambar = v3(0.720, 0.330, 0.062);
    let azul = v3(0.060, 0.120, 0.470);
    let carmin = v3(0.480, 0.062, 0.086);
    let verde = v3(0.072, 0.300, 0.150);
    let oro = v3(0.780, 0.620, 0.220);

    // Nucleo del rosetón.
    if r < 0.075 {
        return if r > 0.058 { plomo } else { oro };
    }
    // Petalos radiales.
    if r < 0.30 {
        let sectores = 8.0;
        let s = (angulo / std::f64::consts::TAU + 0.5) * sectores;
        let indice = s.floor() as i64;
        let ls = s - s.floor();
        // Nervio de plomo entre petalos y aro exterior del rosetón.
        if !(0.06..=0.94).contains(&ls) || r > 0.285 || (r - 0.155).abs() < 0.012 {
            return plomo;
        }
        let anillo_exterior = r > 0.155;
        return match (indice.rem_euclid(4), anillo_exterior) {
            (0, false) => ambar,
            (1, false) => azul,
            (2, false) => carmin,
            (_, false) => verde,
            (0, true) => azul,
            (1, true) => ambar,
            (2, true) => verde,
            (_, true) => carmin,
        };
    }

    // Fondo de rombos alrededor del rosetón.
    let escala = 7.0;
    let du = (u + v) * escala;
    let dv = (u - v) * escala;
    let (fu, fv) = (du - du.floor(), dv - dv.floor());
    let junta = fu.min(1.0 - fu).min(fv.min(1.0 - fv));
    if junta < 0.10 {
        return plomo;
    }
    let id = crate::math::hash01_3(du.floor() as i64, dv.floor() as i64, 0, 0xF00D);
    let panel = if id < 0.34 {
        azul
    } else if id < 0.58 {
        ambar
    } else if id < 0.78 {
        verde
    } else {
        carmin
    };
    // Vidrio soplado: leves variaciones de espesor.
    panel * (0.84 + 0.30 * value2(u * 26.0, v * 26.0, 0xABCD))
}

/// Metal envejecido del escudo y de los herrajes.
fn metal_aged_albedo(u: f64, v: f64) -> Vec3 {
    let bronce = v3(0.700, 0.500, 0.240);
    let bronce_oscuro = v3(0.330, 0.216, 0.100);
    let patina = v3(0.140, 0.300, 0.260);

    let pulido = fbm2(u * 9.0, v * 9.0, 0xB0B0, 3, 2.0, 0.55);
    let mut c = bronce_oscuro.lerp(bronce, pulido);
    // Chorreras verticales de patina.
    let chorrera = ridged2(u * 14.0, v * 3.0, 0xC1C1, 3);
    c = c.lerp(patina, smoothstep((chorrera - 0.46) / 0.34) * 0.72);
    // Picado del metal.
    let pica = cell2(u * 13.0, v * 13.0, 0xD2D2);
    if pica.distance < 0.12 {
        c = c.lerp(v3(0.120, 0.100, 0.070), 0.65);
    }
    // Remaches en las cuatro esquinas de la placa.
    let remache = ((u - 0.15).abs().max((v - 0.15).abs()))
        .min((u - 0.85).abs().max((v - 0.15).abs()))
        .min((u - 0.15).abs().max((v - 0.85).abs()))
        .min((u - 0.85).abs().max((v - 0.85).abs()));
    if remache < 0.045 {
        c = c.lerp(v3(0.660, 0.560, 0.360), 0.8);
    }
    c
}

fn metal_aged_height(x: usize, y: usize) -> f64 {
    let u = (x as f64 + 0.5) / TEX_SIZE as f64;
    let v = (y as f64 + 0.5) / TEX_SIZE as f64;
    let pica = cell2(u * 13.0, v * 13.0, 0xD2D2);
    let hueco = if pica.distance < 0.12 {
        -smoothstep((0.12 - pica.distance) / 0.12) * 0.6
    } else {
        0.0
    };
    let remache = ((u - 0.15).abs().max((v - 0.15).abs()))
        .min((u - 0.85).abs().max((v - 0.15).abs()))
        .min((u - 0.15).abs().max((v - 0.85).abs()))
        .min((u - 0.85).abs().max((v - 0.85).abs()));
    let bulto = if remache < 0.045 {
        smoothstep((0.045 - remache) / 0.045) * 0.9
    } else {
        0.0
    };
    0.22 * fbm2(u * 5.0, v * 5.0, 0xB0B0, 3, 2.0, 0.55) + hueco + bulto
}

/// Farol emisivo: nucleo caliente tras una celosia metalica.
fn lantern_albedo(u: f64, v: f64) -> Vec3 {
    let marco = v3(0.055, 0.045, 0.035);
    // Celosia: marco perimetral y un travesano en cada eje.
    let borde = (u.min(1.0 - u)).min(v.min(1.0 - v));
    if borde < 0.09 || (u - 0.5).abs() < 0.035 || (v - 0.5).abs() < 0.035 {
        return marco;
    }
    let (cx, cy) = (u - 0.5, v - 0.5);
    let r = (cx * cx + cy * cy).sqrt();
    let nucleo = v3(1.00, 0.880, 0.640);
    let ambar = v3(1.00, 0.520, 0.170);
    let caida = smoothstep((r - 0.06) / 0.34);
    let c = nucleo.lerp(ambar, caida);
    // Parpadeo espacial fijo, para que el cristal no parezca liso.
    c * (0.90 + 0.18 * value2(u * 16.0, v * 16.0, 0xFA01))
}

/// Cristal del altar: emisivo, facetado y algo mas frio que el farol.
fn altar_crystal_albedo(u: f64, v: f64) -> Vec3 {
    let c = cell2(u * 4.0, v * 4.0, 0xE1E1);
    let faceta = 0.70 + 0.55 * c.id;
    let calido = v3(1.00, 0.760, 0.400);
    let palido = v3(1.00, 0.920, 0.740);
    let mut color = calido.lerp(palido, smoothstep(c.id));
    color *= faceta;
    // Aristas de las facetas, mas brillantes.
    color = color.lerp(
        v3(1.00, 0.960, 0.860),
        (1.0 - smoothstep(c.border / 0.09)) * 0.8,
    );
    // Vetas internas.
    let vena = ridged2(u * 10.0, v * 10.0, 0xE2E2, 3);
    color * (0.86 + 0.30 * vena)
}

/// Vegetacion discreta: musgo y matas verdes desaturadas.
fn foliage_albedo(u: f64, v: f64) -> Vec3 {
    let hueco = v3(0.038, 0.050, 0.030);
    let hoja_oscura = v3(0.072, 0.128, 0.058);
    let hoja_clara = v3(0.148, 0.218, 0.098);

    let mata = fbm2(u * 11.0, v * 11.0, 0xAB01, 4, 2.2, 0.55);
    let densidad = smoothstep((mata - 0.34) / 0.30);
    let punta = cell2(u * 8.0, v * 8.0, 0xAB02);
    let mut c = hueco.lerp(hoja_oscura.lerp(hoja_clara, punta.id), densidad);
    // Puntas iluminadas en el borde de cada mata.
    c = c.lerp(
        hoja_clara,
        (1.0 - smoothstep(punta.border / 0.10)) * densidad * 0.5,
    );
    c * (0.88 + 0.22 * value2(u * 40.0, v * 40.0, 0xAB03))
}

/// Descripcion de un recurso a generar.
struct Recurso {
    nombre: &'static str,
    tamano: usize,
    albedo: fn(f64, f64) -> Vec3,
    /// Campo de altura y fuerza del relieve, si el material lleva mapa normal.
    normal: Option<Relieve>,
}

/// Campo de altura de un material y la fuerza con la que se convierte en normal.
type Relieve = (fn(usize, usize) -> f64, f64);

/// Todos los recursos de material del proyecto.
fn recursos() -> Vec<Recurso> {
    vec![
        Recurso {
            nombre: "stone_ancient",
            tamano: TEX_SIZE,
            albedo: stone_ancient_albedo,
            normal: Some((stone_ancient_height, 3.2)),
        },
        Recurso {
            nombre: "stone_floor",
            tamano: TEX_SIZE,
            albedo: stone_floor_albedo,
            normal: Some((stone_floor_height, 2.8)),
        },
        Recurso {
            nombre: "stone_rubble",
            tamano: TEX_SIZE,
            albedo: stone_rubble_albedo,
            normal: Some((stone_rubble_height, 2.8)),
        },
        Recurso {
            nombre: "wood_aged",
            tamano: TEX_SIZE,
            albedo: wood_aged_albedo,
            normal: Some((wood_aged_height, 2.6)),
        },
        Recurso {
            nombre: "earth_moss",
            tamano: TEX_SIZE,
            albedo: earth_moss_albedo,
            normal: Some((earth_moss_height, 3.0)),
        },
        Recurso {
            nombre: "earth_dark",
            tamano: TEX_SIZE,
            albedo: earth_dark_albedo,
            normal: None,
        },
        Recurso {
            nombre: "water",
            tamano: TEX_SIZE,
            albedo: water_albedo,
            normal: Some((water_height, 1.6)),
        },
        Recurso {
            nombre: "stained_glass",
            tamano: VITRAL_SIZE,
            albedo: stained_glass_albedo,
            normal: None,
        },
        Recurso {
            nombre: "metal_aged",
            tamano: TEX_SIZE,
            albedo: metal_aged_albedo,
            normal: Some((metal_aged_height, 2.2)),
        },
        Recurso {
            nombre: "lantern_glow",
            tamano: TEX_SIZE,
            albedo: lantern_albedo,
            normal: None,
        },
        Recurso {
            nombre: "altar_crystal",
            tamano: TEX_SIZE,
            albedo: altar_crystal_albedo,
            normal: None,
        },
        Recurso {
            nombre: "foliage",
            tamano: TEX_SIZE,
            albedo: foliage_albedo,
            normal: None,
        },
    ]
}

/// Escala una imagen por un factor entero repitiendo texels.
///
/// Se usa solo para las laminas de documentacion: repetir el texel es la unica
/// ampliacion que no traiciona el aspecto pixelado del original.
pub fn scale_nearest(img: &Image, factor: usize) -> Image {
    let mut salida = Image::new(img.width * factor, img.height * factor);
    for y in 0..salida.height {
        for x in 0..salida.width {
            salida.set(x, y, img.get(x / factor, y / factor));
        }
    }
    salida
}

/// Escribe cada textura ampliada como PNG, para poder mirarlas e incluirlas en la
/// documentacion.
pub fn write_previews(
    assets: &Path,
    destino: &Path,
    factor: usize,
) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(destino)?;
    let dir_tex = assets.join("textures");
    let mut nombres = Vec::new();
    for r in recursos() {
        let mut ficheros = vec![r.nombre.to_string()];
        if r.normal.is_some() {
            ficheros.push(format!("{}_n", r.nombre));
        }
        for f in ficheros {
            let img = Image::read_ppm(dir_tex.join(format!("{f}.ppm")))?;
            let ampliada = scale_nearest(&img, factor);
            ampliada.write_png(destino.join(format!("{f}.png")))?;
            nombres.push(format!("{f}.png"));
        }
    }
    // Una cara del cielo, a tamano natural, como muestra del cubemap.
    let cielo = Image::read_ppm(assets.join("skybox").join("sky_neg_x.ppm"))?;
    cielo.write_png(destino.join("sky_neg_x.png"))?;
    nombres.push("sky_neg_x.png".to_string());
    Ok(nombres)
}

/// Genera todos los recursos y los escribe bajo `assets/`.
///
/// Devuelve la lista de ficheros escritos con su tamano, para poder informar por
/// consola de lo que se ha producido.
pub fn generate_all(assets: &Path) -> std::io::Result<Vec<(String, u64)>> {
    let dir_tex = assets.join("textures");
    let dir_sky = assets.join("skybox");
    std::fs::create_dir_all(&dir_tex)?;
    std::fs::create_dir_all(&dir_sky)?;

    let mut escritos = Vec::new();
    let mut anotar = |ruta: std::path::PathBuf| -> std::io::Result<()> {
        let tamano = std::fs::metadata(&ruta)?.len();
        escritos.push((
            ruta.file_name().unwrap().to_string_lossy().to_string(),
            tamano,
        ));
        Ok(())
    };

    for r in recursos() {
        let img = build(r.tamano, r.albedo);
        let ruta = dir_tex.join(format!("{}.ppm", r.nombre));
        img.write_ppm(&ruta)?;
        anotar(ruta)?;

        if let Some((altura, fuerza)) = r.normal {
            let nm = normal_map(r.tamano, fuerza, altura);
            let ruta = dir_tex.join(format!("{}_n.ppm", r.nombre));
            nm.write_ppm(&ruta)?;
            anotar(ruta)?;
        }
    }

    for (face, nombre) in FACE_NAMES.iter().enumerate() {
        let img = render_face(face, SKY_FACE_SIZE);
        let ruta = dir_sky.join(format!("sky_{nombre}.ppm"));
        img.write_ppm(&ruta)?;
        anotar(ruta)?;
    }

    Ok(escritos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture::{srgb_to_linear, Encoding, Filter, Texture};

    fn textura(f: fn(f64, f64) -> Vec3, size: usize) -> Texture {
        Texture::from_image(&build(size, f), Encoding::Srgb, Filter::Nearest)
    }

    /// Estadisticos de una textura: luminancia media, minima, maxima y la
    /// desviacion tipica, que es la medida de si la superficie tiene variacion o
    /// es plana.
    fn estadisticas(f: fn(f64, f64) -> Vec3, size: usize) -> (f64, f64, f64, f64) {
        let t = textura(f, size);
        let mut vals = Vec::with_capacity(size * size);
        for y in 0..size {
            for x in 0..size {
                let c = t.sample(
                    (x as f64 + 0.5) / size as f64,
                    (y as f64 + 0.5) / size as f64,
                );
                assert!(c.is_finite(), "color no finito");
                vals.push(c.luminance());
            }
        }
        let media = vals.iter().sum::<f64>() / vals.len() as f64;
        let var = vals.iter().map(|v| (v - media).powi(2)).sum::<f64>() / vals.len() as f64;
        let minimo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
        let maximo = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (media, minimo, maximo, var.sqrt())
    }

    #[test]
    fn todas_las_texturas_tienen_variacion_suficiente() {
        // Una desviacion tipica minima descarta superficies planas, que es el
        // defecto que hace que los cubos parezcan de plastico.
        for r in recursos() {
            // El agua se excluye a proposito: su aspecto lo dan la refraccion, el
            // reflejo y el mapa normal, no el color propio, que debe ser casi
            // uniforme. Su variacion se comprueba sobre el relieve, en
            // `los_mapas_normales_estan_normalizados_y_no_estan_invertidos`.
            if r.nombre == "water" {
                continue;
            }
            let (media, _, _, sigma) = estadisticas(r.albedo, r.tamano);
            assert!(
                sigma / media.max(1e-6) > 0.10,
                "{} es demasiado plana: sigma/media = {:.3}",
                r.nombre,
                sigma / media.max(1e-6)
            );
        }
    }

    #[test]
    fn los_albedos_opacos_estan_en_un_rango_fisico() {
        // Ninguna superficie real refleja mas del 90 % ni tan poco que sea negra.
        for nombre in [
            "stone_ancient",
            "stone_floor",
            "stone_rubble",
            "wood_aged",
            "earth_moss",
            "earth_dark",
            "foliage",
        ] {
            let r = recursos().into_iter().find(|r| r.nombre == nombre).unwrap();
            let (media, minimo, maximo, _) = estadisticas(r.albedo, r.tamano);
            assert!(maximo < 0.90, "{nombre} refleja demasiado: {maximo}");
            assert!(minimo > 0.0005, "{nombre} tiene zonas negras: {minimo}");
            assert!(media > 0.008, "{nombre} es demasiado oscura: {media}");
        }
    }

    #[test]
    fn la_paleta_sigue_la_direccion_de_arte() {
        let piedra = textura(stone_ancient_albedo, TEX_SIZE);
        let mut azules = 0;
        let mut total = 0;
        for y in 0..TEX_SIZE {
            for x in 0..TEX_SIZE {
                let c = piedra.sample(
                    (x as f64 + 0.5) / TEX_SIZE as f64,
                    (y as f64 + 0.5) / TEX_SIZE as f64,
                );
                total += 1;
                if c.z >= c.x {
                    azules += 1;
                }
            }
        }
        // Piedra azul grisacea: el azul domina sobre el rojo en la mayoria.
        assert!(azules * 100 / total > 70, "la piedra no es azul grisacea");

        // Tierra marron: el rojo domina sobre el azul.
        let (mut r, mut b) = (0.0, 0.0);
        let tierra = textura(earth_dark_albedo, TEX_SIZE);
        for y in 0..TEX_SIZE {
            for x in 0..TEX_SIZE {
                let c = tierra.sample(
                    (x as f64 + 0.5) / TEX_SIZE as f64,
                    (y as f64 + 0.5) / TEX_SIZE as f64,
                );
                r += c.x;
                b += c.z;
            }
        }
        assert!(r > b * 1.4, "la tierra deberia ser marron");

        // Musgo verde desaturado: el verde domina pero sin saturarse.
        let hoja = textura(foliage_albedo, TEX_SIZE);
        let c = hoja.sample(0.5, 0.5);
        let media_hoja = (c.x + c.y + c.z) / 3.0;
        assert!(c.y > c.x && c.y > c.z, "la vegetacion deberia ser verde");
        assert!(c.y < media_hoja * 2.2, "el verde esta demasiado saturado");
    }

    #[test]
    fn los_emisivos_son_calidos_y_brillantes() {
        for f in [
            lantern_albedo as fn(f64, f64) -> Vec3,
            altar_crystal_albedo as fn(f64, f64) -> Vec3,
        ] {
            let (media, _, maximo, _) = estadisticas(f, TEX_SIZE);
            assert!(maximo > 0.6, "el emisor deberia tener un nucleo brillante");
            assert!(media > 0.15);
            let t = textura(f, TEX_SIZE);
            let c = t.sample(0.32, 0.32);
            assert!(c.x >= c.z, "el emisor deberia ser calido: {c:?}");
        }
    }

    #[test]
    fn el_vitral_tiene_plomo_y_paneles_de_varios_colores() {
        let t = textura(stained_glass_albedo, VITRAL_SIZE);
        let mut plomo = 0;
        let mut calidos = 0;
        let mut frios = 0;
        for y in 0..VITRAL_SIZE {
            for x in 0..VITRAL_SIZE {
                let c = t.sample(
                    (x as f64 + 0.5) / VITRAL_SIZE as f64,
                    (y as f64 + 0.5) / VITRAL_SIZE as f64,
                );
                if c.luminance() < 0.06 {
                    plomo += 1;
                } else if c.x > c.z * 1.5 {
                    calidos += 1;
                } else if c.z > c.x * 1.5 {
                    frios += 1;
                }
            }
        }
        let total = VITRAL_SIZE * VITRAL_SIZE;
        assert!(plomo * 100 / total > 8, "la celosia de plomo no se ve");
        assert!(plomo * 100 / total < 45, "hay demasiado plomo");
        assert!(calidos * 100 / total > 12, "faltan paneles calidos");
        assert!(frios * 100 / total > 8, "faltan paneles frios");
    }

    #[test]
    fn el_metal_es_suficientemente_claro_para_reflejar() {
        let (media, _, _, _) = estadisticas(metal_aged_albedo, TEX_SIZE);
        assert!(media > 0.10, "el metal quedaria negro: {media}");
    }

    #[test]
    fn los_mapas_normales_estan_normalizados_y_no_estan_invertidos() {
        for r in recursos() {
            let Some((altura, fuerza)) = r.normal else {
                continue;
            };
            let img = normal_map(r.tamano, fuerza, altura);
            let t = Texture::from_image(&img, Encoding::Linear, Filter::Nearest);
            let mut suma_z = 0.0;
            let mut desviacion = 0.0;
            for y in 0..r.tamano {
                for x in 0..r.tamano {
                    let u = (x as f64 + 0.5) / r.tamano as f64;
                    let v = (y as f64 + 0.5) / r.tamano as f64;
                    let n = t.sample_normal(u, v);
                    assert!((n.length() - 1.0).abs() < 1e-9, "{} no unitaria", r.nombre);
                    assert!(n.z > 0.0, "{} mira hacia dentro", r.nombre);
                    suma_z += n.z;
                    desviacion += (n.x * n.x + n.y * n.y).sqrt();
                }
            }
            let n = (r.tamano * r.tamano) as f64;
            // Tiene relieve de verdad, pero no tanto como para parecer ruido. El
            // tope superior es tan importante como el inferior: un mapa saturado
            // en todos los texels no se lee como relieve, se lee como grano.
            let inclinacion = desviacion / n;
            assert!(
                inclinacion > 0.05,
                "{} apenas tiene relieve: {inclinacion:.3}",
                r.nombre
            );
            assert!(
                inclinacion < 0.40,
                "{} esta saturada de relieve: {inclinacion:.3}",
                r.nombre
            );
            assert!(suma_z / n > 0.80, "{} esta demasiado abollada", r.nombre);
        }
    }

    #[test]
    fn el_mapa_normal_responde_a_la_pendiente_del_campo_de_altura() {
        // Rampa que sube hacia +u: la normal debe inclinarse hacia -u.
        let rampa = |x: usize, _y: usize| x as f64 / 16.0;
        let img = normal_map(16, 16.0, rampa);
        let t = Texture::from_image(&img, Encoding::Linear, Filter::Nearest);
        let n = t.sample_normal(0.5, 0.5);
        assert!(n.x < -0.3, "la normal deberia inclinarse hacia -u: {n:?}");
        assert!(n.y.abs() < 0.05);

        // Rampa que sube hacia +v: la normal se inclina hacia -v.
        let rampa_v = |_x: usize, y: usize| y as f64 / 16.0;
        let img = normal_map(16, 16.0, rampa_v);
        let t = Texture::from_image(&img, Encoding::Linear, Filter::Nearest);
        let n = t.sample_normal(0.5, 0.5);
        assert!(n.y < -0.3, "la normal deberia inclinarse hacia -v: {n:?}");
    }

    #[test]
    fn el_mapa_normal_del_agua_se_repite_sin_costura() {
        // El campo de altura del agua es periodico en las dos direcciones, asi que
        // los bordes opuestos deben coincidir: es lo que evita la retícula visible
        // sobre el estanque.
        let n = TEX_SIZE;
        let mut peor = 0.0f64;
        for i in 0..n {
            let izquierda = water_height(0, i);
            let derecha_virtual = {
                // El pixel que seguiria al ultimo es de nuevo el primero.
                let u = 0.5 / n as f64 + 1.0;
                let v = (i as f64 + 0.5) / n as f64;
                let tau = std::f64::consts::TAU;
                let a = (tau * (2.0 * u + 0.30 * (tau * v).sin())).sin();
                let b = (tau * (3.0 * v - 0.22 * (tau * 2.0 * u).sin())).sin();
                let c = (tau * (u + v)).sin();
                0.5 * a + 0.32 * b + 0.18 * c
            };
            peor = peor.max((izquierda - derecha_virtual).abs());
        }
        assert!(peor < 1e-9, "el agua no se repite sin costura: {peor}");
    }

    #[test]
    fn las_juntas_de_la_piedra_son_visibles_y_estan_hundidas() {
        // El mortero debe leerse en el albedo y tambien en el relieve.
        let junta = stone_ancient_albedo(0.5, 0.25);
        let sillar = stone_ancient_albedo(0.25, 0.12);
        assert!(
            junta.luminance() < sillar.luminance() * 0.75,
            "la junta no contrasta: {junta:?} vs {sillar:?}"
        );
        let h_junta = stone_ancient_height(TEX_SIZE / 2, TEX_SIZE / 4);
        let h_sillar = stone_ancient_height(TEX_SIZE / 4, 3);
        assert!(h_junta < h_sillar, "la junta deberia estar hundida");
    }

    #[test]
    fn la_madera_muestra_veta_a_lo_largo_de_u() {
        // La variacion recorriendo v debe ser mayor que recorriendo u, porque las
        // lineas de veta son paralelas a u.
        let variacion = |a: f64, cruzado: bool| {
            let mut vals = Vec::new();
            for i in 0..64 {
                let t = (i as f64 + 0.5) / 64.0;
                let c = if cruzado {
                    wood_aged_albedo(a, t)
                } else {
                    wood_aged_albedo(t, a)
                };
                vals.push(c.luminance());
            }
            let m = vals.iter().sum::<f64>() / vals.len() as f64;
            (vals.iter().map(|v| (v - m).powi(2)).sum::<f64>() / vals.len() as f64).sqrt()
        };
        let a_lo_largo = variacion(0.375, false);
        let a_lo_ancho = variacion(0.375, true);
        assert!(
            a_lo_ancho > a_lo_largo,
            "la veta no se lee: a lo ancho {a_lo_ancho:.4}, a lo largo {a_lo_largo:.4}"
        );
    }

    #[test]
    fn la_generacion_es_reproducible() {
        let a = build(TEX_SIZE, stone_ancient_albedo);
        let b = build(TEX_SIZE, stone_ancient_albedo);
        assert_eq!(a, b);
        let c = normal_map(TEX_SIZE, 9.0, stone_ancient_height);
        let d = normal_map(TEX_SIZE, 9.0, stone_ancient_height);
        assert_eq!(c, d);
    }

    #[test]
    fn la_codificacion_de_ida_y_vuelta_conserva_el_color() {
        // El paso por sRGB de ocho bits no debe desplazar el albedo mas de un 2 %.
        for r in recursos() {
            let img = build(r.tamano, r.albedo);
            let t = Texture::from_image(&img, Encoding::Srgb, Filter::Nearest);
            let mut peor = 0.0f64;
            for y in (0..r.tamano).step_by(3) {
                for x in (0..r.tamano).step_by(3) {
                    let u = (x as f64 + 0.5) / r.tamano as f64;
                    let v = (y as f64 + 0.5) / r.tamano as f64;
                    let original = (r.albedo)(u, v);
                    let recuperado = t.sample(u, v);
                    // Se compara en espacio percibido, que es donde vive el error
                    // de cuantizacion de ocho bits.
                    for k in 0..3 {
                        let o = crate::texture::linear_to_srgb(original.axis(k));
                        let d = crate::texture::linear_to_srgb(recuperado.axis(k));
                        peor = peor.max((o - d).abs());
                    }
                }
            }
            assert!(peor < 0.02, "{} se desvia {peor}", r.nombre);
        }
        // Y la conversion en si es exacta hasta el bit.
        assert!((srgb_to_linear(linear_to_srgb(0.37)) - 0.37).abs() < 1e-9);
    }
}
