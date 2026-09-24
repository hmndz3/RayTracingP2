//! Algebra vectorial minima implementada a mano.
//!
//! Se usa `f64` en todo el trazador: la rejilla voxel exige comparaciones
//! estables al cruzar planos enteros y la precision simple producia artefactos
//! de auto-interseccion en los bordes de las celdas.

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub};

/// Vector de tres componentes, usado para posiciones, direcciones y color lineal.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Constructor breve.
#[inline]
pub const fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

impl Vec3 {
    pub const ZERO: Vec3 = v3(0.0, 0.0, 0.0);
    pub const ONE: Vec3 = v3(1.0, 1.0, 1.0);

    #[inline]
    pub const fn splat(s: f64) -> Vec3 {
        v3(s, s, s)
    }

    #[inline]
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    #[inline]
    pub fn cross(self, o: Vec3) -> Vec3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    #[inline]
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    #[inline]
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    /// Normaliza el vector. Devuelve el eje Y si la longitud es degenerada, de
    /// modo que ningun rayo secundario pueda propagar un `NaN` por la escena.
    #[inline]
    pub fn normalized(self) -> Vec3 {
        let len2 = self.length_squared();
        if len2 <= 1e-30 {
            v3(0.0, 1.0, 0.0)
        } else {
            self * len2.sqrt().recip()
        }
    }

    /// Producto componente a componente, la operacion natural para filtrar color.
    #[inline]
    pub fn mul_elem(self, o: Vec3) -> Vec3 {
        v3(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    #[inline]
    pub fn min_elem(self, o: Vec3) -> Vec3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    #[inline]
    pub fn max_elem(self, o: Vec3) -> Vec3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }

    #[inline]
    pub fn max_component(self) -> f64 {
        self.x.max(self.y).max(self.z)
    }

    /// Luminancia relativa en espacio lineal (coeficientes Rec. 709).
    #[inline]
    pub fn luminance(self) -> f64 {
        0.2126 * self.x + 0.7152 * self.y + 0.0722 * self.z
    }

    #[inline]
    pub fn axis(self, i: usize) -> f64 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }

    #[inline]
    pub fn set_axis(&mut self, i: usize, value: f64) {
        match i {
            0 => self.x = value,
            1 => self.y = value,
            _ => self.z = value,
        }
    }

    /// Reciproco por componente, tolerante al cero: los rayos paralelos a una
    /// cara reciben un infinito con signo y el test de rebanadas sigue siendo
    /// correcto sin ramificaciones adicionales.
    #[inline]
    pub fn recip_safe(self) -> Vec3 {
        #[inline]
        fn r(a: f64) -> f64 {
            if a.abs() < 1e-18 {
                if a.is_sign_negative() {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            } else {
                1.0 / a
            }
        }
        v3(r(self.x), r(self.y), r(self.z))
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Verdadero si el vector esta practicamente en el origen.
    #[inline]
    pub fn near_zero(self) -> bool {
        const E: f64 = 1e-9;
        self.x.abs() < E && self.y.abs() < E && self.z.abs() < E
    }

    #[inline]
    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self * (1.0 - t) + o * t
    }

    #[inline]
    pub fn clamp01(self) -> Vec3 {
        v3(
            self.x.clamp(0.0, 1.0),
            self.y.clamp(0.0, 1.0),
            self.z.clamp(0.0, 1.0),
        )
    }

    #[inline]
    pub fn powf(self, e: f64) -> Vec3 {
        v3(self.x.powf(e), self.y.powf(e), self.z.powf(e))
    }

    /// `exp(-self)` componente a componente, la forma que toma la ley de
    /// Beer-Lambert al atenuar un rayo dentro de un medio absorbente.
    #[inline]
    pub fn exp_neg(self) -> Vec3 {
        v3((-self.x).exp(), (-self.y).exp(), (-self.z).exp())
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    #[inline]
    fn add(self, o: Vec3) -> Vec3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for Vec3 {
    #[inline]
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    #[inline]
    fn sub(self, o: Vec3) -> Vec3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn mul(self, s: f64) -> Vec3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}

impl Mul<Vec3> for f64 {
    type Output = Vec3;
    #[inline]
    fn mul(self, v: Vec3) -> Vec3 {
        v * self
    }
}

impl MulAssign<f64> for Vec3 {
    #[inline]
    fn mul_assign(&mut self, s: f64) {
        *self = *self * s;
    }
}

impl Div<f64> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn div(self, s: f64) -> Vec3 {
        self * (1.0 / s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    #[inline]
    fn neg(self) -> Vec3 {
        v3(-self.x, -self.y, -self.z)
    }
}

/// Base ortonormal construida alrededor de una normal, usada para transformar
/// las normales leidas del mapa (espacio tangente) al espacio de la escena y
/// para generar direcciones sobre el hemisferio.
#[derive(Debug, Clone, Copy)]
pub struct Onb {
    pub tangent: Vec3,
    pub bitangent: Vec3,
    pub normal: Vec3,
}

impl Onb {
    /// Base derecha a partir de una normal, eligiendo un eje auxiliar que no sea
    /// casi paralelo para no degenerar el producto cruz.
    pub fn from_normal(normal: Vec3) -> Onb {
        let n = normal.normalized();
        let aux = if n.x.abs() < 0.9 {
            v3(1.0, 0.0, 0.0)
        } else {
            v3(0.0, 1.0, 0.0)
        };
        let bitangent = n.cross(aux).normalized();
        let tangent = bitangent.cross(n);
        Onb {
            tangent,
            bitangent,
            normal: n,
        }
    }

    /// Lleva un vector del espacio tangente al espacio de la escena.
    #[inline]
    pub fn to_world(&self, local: Vec3) -> Vec3 {
        self.tangent * local.x + self.bitangent * local.y + self.normal * local.z
    }
}

/// Reflexion especular ideal. `n` debe apuntar hacia el lado de `d`.
#[inline]
pub fn reflect(d: Vec3, n: Vec3) -> Vec3 {
    d - n * (2.0 * d.dot(n))
}

/// Refraccion por la ley de Snell.
///
/// `d` es la direccion incidente normalizada, `n` la normal orientada contra `d`
/// y `eta` la razon `n1 / n2`. Devuelve `None` cuando el discriminante es
/// negativo, es decir en reflexion interna total: quien llama debe reflejar.
#[inline]
pub fn refract(d: Vec3, n: Vec3, eta: f64) -> Option<Vec3> {
    let cos_i = (-d).dot(n).clamp(-1.0, 1.0);
    let sin2_t = eta * eta * (1.0 - cos_i * cos_i);
    if sin2_t > 1.0 {
        return None;
    }
    let cos_t = (1.0 - sin2_t).max(0.0).sqrt();
    Some(d * eta + n * (eta * cos_i - cos_t))
}

/// Aproximacion de Schlick al coeficiente de Fresnel para un dielectrico.
///
/// `cos_i` es el coseno del angulo de incidencia medido contra la normal
/// orientada hacia el rayo. `F0` se calcula con los indices reales para que el
/// valor sea el mismo al entrar y al salir del medio.
#[inline]
pub fn fresnel_schlick_dielectric(cos_i: f64, n1: f64, n2: f64) -> f64 {
    let r0 = ((n1 - n2) / (n1 + n2)).powi(2);
    let c = cos_i.clamp(0.0, 1.0);
    r0 + (1.0 - r0) * (1.0 - c).powi(5)
}

/// Schlick con reflectancia normal explicita, para metales y realces tenues.
#[inline]
pub fn fresnel_schlick_f0(cos_i: f64, f0: Vec3) -> Vec3 {
    let c = (1.0 - cos_i.clamp(0.0, 1.0)).powi(5);
    f0 + (Vec3::ONE - f0) * c
}

/// Interpolacion suave de Hermite, usada por varias mascaras de material.
#[inline]
pub fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Quintica de Perlin: primera y segunda derivada nulas en los extremos, lo que
/// elimina las bandas visibles en el terreno interpolado.
#[inline]
pub fn smootherstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

#[inline]
pub fn degrees_to_radians(d: f64) -> f64 {
    d * std::f64::consts::PI / 180.0
}

/// Mezclador entero de 64 bits (constantes de SplitMix64). Es la base de todo el
/// azar del proyecto: siendo una funcion pura de la semilla y las coordenadas,
/// el terreno y el grano de las texturas son reproducibles bit a bit.
#[inline]
pub fn hash_u64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Escalar reproducible en `[0, 1)` a partir de tres enteros y una semilla.
#[inline]
pub fn hash01_3(x: i64, y: i64, z: i64, seed: u64) -> f64 {
    let h = hash_u64(
        (x as u64).wrapping_mul(0x1F1F_1F1F_1F1F_1F1F)
            ^ (y as u64).wrapping_mul(0x27D4_EB2F_1656_67C5)
            ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
            ^ seed,
    );
    (h >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
}

/// Generador xorshift para el muestreo dentro del trazador. Cada bloque de
/// pixeles siembra su propia secuencia, asi que no hay estado compartido entre
/// hilos ni necesidad de sincronizacion.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng {
            state: hash_u64(seed ^ 0xA076_1D64_78BD_642F).max(1),
        }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Flotante uniforme en `[0, 1)`.
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    #[inline]
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// Punto uniforme sobre la esfera unitaria, por rechazo.
    pub fn unit_vector(&mut self) -> Vec3 {
        loop {
            let p = v3(
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
            );
            let l2 = p.length_squared();
            if l2 > 1e-6 && l2 <= 1.0 {
                return p * l2.sqrt().recip();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn producto_cruz_es_derecho() {
        let x = v3(1.0, 0.0, 0.0);
        let y = v3(0.0, 1.0, 0.0);
        assert_eq!(x.cross(y), v3(0.0, 0.0, 1.0));
    }

    #[test]
    fn normalizar_vector_degenerado_no_produce_nan() {
        let n = Vec3::ZERO.normalized();
        assert!(n.is_finite());
        assert!((n.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn reflexion_invierte_la_componente_normal() {
        let d = v3(1.0, -1.0, 0.0).normalized();
        let n = v3(0.0, 1.0, 0.0);
        let r = reflect(d, n);
        assert!((r.y + d.y).abs() < 1e-12);
        assert!((r.x - d.x).abs() < 1e-12);
        assert!((r.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn refraccion_cumple_la_ley_de_snell() {
        // Aire hacia vidrio, incidencia de 45 grados.
        let n = v3(0.0, 1.0, 0.0);
        let theta_i = degrees_to_radians(45.0);
        let d = v3(theta_i.sin(), -theta_i.cos(), 0.0);
        let (n1, n2) = (1.0, 1.52);
        let t = refract(d, n, n1 / n2).expect("debe refractar");
        let theta_t = (t.x / t.length()).asin();
        assert!((n1 * theta_i.sin() - n2 * theta_t.sin()).abs() < 1e-9);
        // El rayo transmitido se acerca a la normal al entrar al medio denso.
        assert!(theta_t < theta_i);
    }

    #[test]
    fn reflexion_interna_total_por_encima_del_angulo_critico() {
        let n = v3(0.0, 1.0, 0.0);
        let (n1, n2): (f64, f64) = (1.52, 1.0);
        let critico = (n2 / n1).asin();
        let bajo = critico - degrees_to_radians(2.0);
        let sobre = critico + degrees_to_radians(2.0);
        let rayo = |theta: f64| v3(theta.sin(), -theta.cos(), 0.0);
        assert!(refract(rayo(bajo), n, n1 / n2).is_some());
        assert!(refract(rayo(sobre), n, n1 / n2).is_none());
    }

    #[test]
    fn fresnel_es_simetrico_y_tiende_a_uno_en_rasante() {
        let a = fresnel_schlick_dielectric(1.0, 1.0, 1.52);
        let b = fresnel_schlick_dielectric(1.0, 1.52, 1.0);
        assert!((a - b).abs() < 1e-12);
        assert!(a > 0.03 && a < 0.05);
        assert!(fresnel_schlick_dielectric(0.0, 1.0, 1.52) > 0.999);
    }

    #[test]
    fn base_tangente_es_ortonormal_y_derecha() {
        for n in [v3(0.0, 1.0, 0.0), v3(0.3, -0.5, 0.8), v3(-1.0, 0.0, 0.0)] {
            let b = Onb::from_normal(n);
            assert!(b.tangent.dot(b.bitangent).abs() < 1e-12);
            assert!(b.tangent.dot(b.normal).abs() < 1e-12);
            assert!((b.tangent.cross(b.bitangent) - b.normal).length() < 1e-9);
        }
    }

    #[test]
    fn reciproco_seguro_gestiona_rayos_paralelos() {
        let r = v3(0.0, -0.0, 2.0).recip_safe();
        assert!(r.x.is_infinite() && r.x > 0.0);
        assert!(r.y.is_infinite() && r.y < 0.0);
        assert!((r.z - 0.5).abs() < 1e-12);
    }

    #[test]
    fn el_hash_es_reproducible_y_esta_bien_repartido() {
        assert_eq!(hash01_3(3, 4, 5, 77), hash01_3(3, 4, 5, 77));
        assert_ne!(hash01_3(3, 4, 5, 77), hash01_3(3, 4, 5, 78));
        let mut suma = 0.0;
        let n = 20_000;
        for i in 0..n {
            let v = hash01_3(i as i64, 17, -3, 1);
            assert!((0.0..1.0).contains(&v));
            suma += v;
        }
        assert!((suma / n as f64 - 0.5).abs() < 0.01);
    }

    #[test]
    fn rng_produce_vectores_unitarios() {
        let mut rng = Rng::new(12345);
        for _ in 0..1000 {
            assert!((rng.unit_vector().length() - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn beer_lambert_atenua_monotonicamente() {
        let absorcion = v3(0.2, 0.05, 0.02);
        let cerca = (absorcion * 1.0).exp_neg();
        let lejos = (absorcion * 4.0).exp_neg();
        assert!(lejos.x < cerca.x && cerca.x < 1.0);
        // El canal rojo se absorbe mas rapido: el medio tiende a azul verdoso.
        assert!(lejos.x < lejos.z);
    }
}
