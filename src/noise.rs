//! Ruido procedural reproducible, compartido por el generador de texturas y por
//! el terreno.
//!
//! Todo se construye sobre [`crate::math::hash_u64`]: como el ruido es una funcion
//! pura de las coordenadas y de la semilla, no hay tablas de permutacion que
//! inicializar, el resultado es identico en cualquier maquina y basta con guardar
//! la semilla para poder regenerar el diorama bit a bit.

use crate::math::{hash01_3, smootherstep};

/// Ruido de valor en dos dimensiones, interpolado con la quintica de Perlin.
///
/// Devuelve un valor en `[0, 1]`. Se interpola con la quintica y no linealmente
/// porque la interpolacion lineal deja visibles las aristas de la retícula en
/// forma de facetas, algo que en el terreno se nota como escalones rectos.
pub fn value2(x: f64, y: f64, seed: u64) -> f64 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (smootherstep(x - xi), smootherstep(y - yi));
    let (ix, iy) = (xi as i64, yi as i64);

    let v = |dx: i64, dy: i64| hash01_3(ix + dx, iy + dy, 0, seed);
    let a = v(0, 0) + (v(1, 0) - v(0, 0)) * fx;
    let b = v(0, 1) + (v(1, 1) - v(0, 1)) * fx;
    a + (b - a) * fy
}

/// Ruido de valor en tres dimensiones, en `[0, 1]`.
pub fn value3(x: f64, y: f64, z: f64, seed: u64) -> f64 {
    let (xi, yi, zi) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (
        smootherstep(x - xi),
        smootherstep(y - yi),
        smootherstep(z - zi),
    );
    let (ix, iy, iz) = (xi as i64, yi as i64, zi as i64);

    let v = |dx: i64, dy: i64, dz: i64| hash01_3(ix + dx, iy + dy, iz + dz, seed);
    let mezcla = |a: f64, b: f64, t: f64| a + (b - a) * t;

    let x00 = mezcla(v(0, 0, 0), v(1, 0, 0), fx);
    let x10 = mezcla(v(0, 1, 0), v(1, 1, 0), fx);
    let x01 = mezcla(v(0, 0, 1), v(1, 0, 1), fx);
    let x11 = mezcla(v(0, 1, 1), v(1, 1, 1), fx);
    let y0 = mezcla(x00, x10, fy);
    let y1 = mezcla(x01, x11, fy);
    mezcla(y0, y1, fz)
}

/// Suma de octavas de ruido de valor, normalizada a `[0, 1]`.
///
/// `lacunarity` multiplica la frecuencia en cada octava y `gain` su amplitud. La
/// normalizacion por la suma de amplitudes es lo que permite cambiar el numero de
/// octavas sin que el terreno cambie de escala vertical.
pub fn fbm2(x: f64, y: f64, seed: u64, octaves: u32, lacunarity: f64, gain: f64) -> f64 {
    let mut suma = 0.0;
    let mut amplitud = 1.0;
    let mut total = 0.0;
    let mut frecuencia = 1.0;
    for o in 0..octaves {
        suma += amplitud
            * value2(
                x * frecuencia,
                y * frecuencia,
                seed ^ (o as u64 * 0x9E37_79B9),
            );
        total += amplitud;
        amplitud *= gain;
        frecuencia *= lacunarity;
    }
    if total > 0.0 {
        suma / total
    } else {
        0.0
    }
}

/// Variante de tres dimensiones de [`fbm2`].
pub fn fbm3(x: f64, y: f64, z: f64, seed: u64, octaves: u32, lacunarity: f64, gain: f64) -> f64 {
    let mut suma = 0.0;
    let mut amplitud = 1.0;
    let mut total = 0.0;
    let mut f = 1.0;
    for o in 0..octaves {
        suma += amplitud * value3(x * f, y * f, z * f, seed ^ (o as u64 * 0x85EB_CA6B));
        total += amplitud;
        amplitud *= gain;
        f *= lacunarity;
    }
    if total > 0.0 {
        suma / total
    } else {
        0.0
    }
}

/// Ruido crestado: pliega el ruido alrededor de `0.5` y realza los maximos.
///
/// Produce lineas finas y continuas en lugar de manchas, que es la forma que
/// tienen las vetas de la madera y las grietas de la piedra.
pub fn ridged2(x: f64, y: f64, seed: u64, octaves: u32) -> f64 {
    let mut suma = 0.0;
    let mut amplitud = 1.0;
    let mut total = 0.0;
    let mut f = 1.0;
    for o in 0..octaves {
        let n = 1.0 - (value2(x * f, y * f, seed ^ (o as u64 * 0xC2B2_AE35)) * 2.0 - 1.0).abs();
        suma += amplitud * n * n;
        total += amplitud;
        amplitud *= 0.5;
        f *= 2.0;
    }
    if total > 0.0 {
        suma / total
    } else {
        0.0
    }
}

/// Resultado del ruido celular: distancia al punto mas cercano y su identificador.
pub struct Cell2 {
    /// Distancia euclidea al punto sembrado mas cercano, en unidades de celda.
    pub distance: f64,
    /// Diferencia entre la segunda y la primera distancia: vale casi cero
    /// justo sobre la frontera entre dos celdas, lo que la convierte en la
    /// mascara natural para dibujar juntas de mortero.
    pub border: f64,
    /// Escalar estable en `[0, 1)` asociado a la celda ganadora, para variar el
    /// tono de cada piedra sin romper su contorno.
    pub id: f64,
}

/// Ruido celular de Worley con una semilla por celda de la retícula entera.
pub fn cell2(x: f64, y: f64, seed: u64) -> Cell2 {
    let (ix, iy) = (x.floor() as i64, y.floor() as i64);
    let mut d1 = f64::INFINITY;
    let mut d2 = f64::INFINITY;
    let mut id = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (ix + dx, iy + dy);
            let jx = hash01_3(cx, cy, 11, seed);
            let jy = hash01_3(cx, cy, 23, seed);
            let px = cx as f64 + jx;
            let py = cy as f64 + jy;
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = hash01_3(cx, cy, 37, seed);
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    Cell2 {
        distance: d1,
        border: d2 - d1,
        id,
    }
}

/// Ruido de valor sobre una direccion unitaria, continuo en toda la esfera.
///
/// Se evalua en el espacio tridimensional de la propia direccion, no en
/// coordenadas de cara ni en latitud y longitud. Por eso el resultado solo
/// depende de la direccion y no de que cara del cubemap se este rellenando: es lo
/// que garantiza que las seis caras encajen sin costura.
pub fn directional(dir: crate::math::Vec3, escala: f64, seed: u64, octaves: u32) -> f64 {
    let d = dir.normalized() * escala;
    fbm3(d.x, d.y, d.z, seed, octaves, 2.0, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v3;

    #[test]
    fn el_ruido_esta_acotado_y_es_reproducible() {
        for i in 0..2000 {
            let x = i as f64 * 0.137;
            let y = i as f64 * 0.041;
            let a = value2(x, y, 9);
            assert!((0.0..=1.0).contains(&a), "value2 fuera de rango: {a}");
            assert_eq!(a, value2(x, y, 9));
            let b = value3(x, y, x - y, 9);
            assert!((0.0..=1.0).contains(&b));
            let c = fbm2(x, y, 9, 5, 2.0, 0.5);
            assert!((0.0..=1.0).contains(&c), "fbm2 fuera de rango: {c}");
            let d = ridged2(x, y, 9, 4);
            assert!((0.0..=1.0).contains(&d), "ridged2 fuera de rango: {d}");
        }
    }

    #[test]
    fn el_ruido_cambia_con_la_semilla() {
        let a: Vec<f64> = (0..50).map(|i| value2(i as f64 * 0.3, 1.7, 1)).collect();
        let b: Vec<f64> = (0..50).map(|i| value2(i as f64 * 0.3, 1.7, 2)).collect();
        assert!(
            a.iter()
                .zip(&b)
                .filter(|(x, y)| (*x - *y).abs() > 1e-6)
                .count()
                > 40
        );
    }

    #[test]
    fn el_ruido_es_continuo_al_cruzar_la_reticula() {
        // Al acercarse a un nodo entero por los dos lados el valor debe coincidir.
        for k in 1..20 {
            let x = k as f64;
            let izquierda = value2(x - 1e-9, 0.37, 5);
            let derecha = value2(x + 1e-9, 0.37, 5);
            assert!((izquierda - derecha).abs() < 1e-6, "salto en x={x}");
        }
    }

    #[test]
    fn el_ruido_interpola_los_valores_de_los_nodos() {
        // Justo sobre un nodo, el ruido devuelve el valor del propio nodo.
        let esperado = crate::math::hash01_3(3, 4, 0, 77);
        assert!((value2(3.0, 4.0, 77) - esperado).abs() < 1e-12);
    }

    #[test]
    fn mas_octavas_anaden_detalle_sin_cambiar_la_escala() {
        let media = |oct: u32| {
            let n = 4000;
            let s: f64 = (0..n)
                .map(|i| fbm2(i as f64 * 0.013, i as f64 * 0.029, 3, oct, 2.0, 0.5))
                .sum();
            s / n as f64
        };
        let (m2, m6) = (media(2), media(6));
        assert!((m2 - 0.5).abs() < 0.08 && (m6 - 0.5).abs() < 0.08);
        assert!((m2 - m6).abs() < 0.06, "la escala cambia con las octavas");
    }

    #[test]
    fn el_ruido_celular_marca_las_fronteras() {
        let mut minimo_borde = f64::INFINITY;
        for i in 0..400 {
            let c = cell2(i as f64 * 0.05, i as f64 * 0.031, 4);
            assert!(c.distance >= 0.0 && c.distance < 3.0);
            assert!(c.border >= 0.0);
            assert!((0.0..1.0).contains(&c.id));
            minimo_borde = minimo_borde.min(c.border);
        }
        // En algun punto del muestreo se pasa muy cerca de una frontera.
        assert!(minimo_borde < 0.02, "no se encontro ninguna junta");
    }

    #[test]
    fn el_ruido_direccional_solo_depende_de_la_direccion() {
        // La misma direccion con distinta longitud da el mismo valor: es la
        // propiedad de la que depende que el cubemap no tenga costuras.
        for i in 0..200 {
            let d = v3(
                (i as f64 * 0.7).sin(),
                (i as f64 * 0.31).cos(),
                (i as f64 * 1.3).sin(),
            );
            let a = directional(d, 3.0, 8, 3);
            let b = directional(d * 17.5, 3.0, 8, 3);
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn el_ruido_direccional_es_continuo_en_las_aristas_del_cubo() {
        // Dos caras contiguas evaluan la misma direccion en su arista comun.
        let pasos = 64;
        for i in 0..=pasos {
            let t = -1.0 + 2.0 * i as f64 / pasos as f64;
            // Arista entre +X y +Y: direccion (1, 1, t).
            let desde_x = directional(v3(1.0, 1.0, t), 4.0, 12, 4);
            let desde_y = directional(v3(1.0, 1.0, t), 4.0, 12, 4);
            assert_eq!(desde_x, desde_y);
            // Aproximarse a la arista desde el interior de cada cara converge.
            let cerca_x = directional(v3(1.0, 0.999, t), 4.0, 12, 4);
            let cerca_y = directional(v3(0.999, 1.0, t), 4.0, 12, 4);
            assert!((cerca_x - desde_x).abs() < 0.01);
            assert!((cerca_y - desde_y).abs() < 0.01);
        }
    }
}
