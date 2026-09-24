//! Rayos, intervalos parametricos y el estado del medio que atraviesa un rayo.

use crate::math::{v3, Vec3};

/// Rayo con el reciproco de la direccion precalculado.
///
/// Guardar `inv_dir` ahorra tres divisiones por cada caja que se prueba y por
/// cada plano de celda que cruza el recorrido DDA, que es el bucle mas caliente
/// de todo el trazador.
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
    pub inv_dir: Vec3,
}

impl Ray {
    /// Construye un rayo normalizando la direccion.
    ///
    /// La normalizacion permite que `t` sea una distancia real, lo que hace
    /// directamente utilizable la ley de Beer-Lambert y los cortes por radio de
    /// influencia de las luces.
    pub fn new(origin: Vec3, dir: Vec3) -> Ray {
        let dir = dir.normalized();
        Ray {
            origin,
            dir,
            inv_dir: dir.recip_safe(),
        }
    }

    #[inline]
    pub fn at(&self, t: f64) -> Vec3 {
        self.origin + self.dir * t
    }
}

/// Desplazamiento aplicado al origen de todo rayo secundario.
///
/// La geometria vive sobre una retícula entera, asi que el error absoluto de las
/// coordenadas de impacto es del orden de 1e-13; 1e-4 esta muy por encima del
/// ruido y muy por debajo de una celda, de modo que no se producen ni
/// auto-intersecciones ni fugas de luz por las juntas.
pub const SHADOW_EPSILON: f64 = 1e-4;

/// Crea un rayo secundario separado de la superficie a lo largo de la normal.
///
/// El signo se elige con la direccion de salida y no con la cara impactada, para
/// que funcione igual al reflejar (mismo lado) y al refractar (lado opuesto).
pub fn offset_ray(point: Vec3, normal: Vec3, dir: Vec3) -> Ray {
    let sign = if dir.dot(normal) < 0.0 { -1.0 } else { 1.0 };
    Ray::new(point + normal * (SHADOW_EPSILON * sign), dir)
}

/// Intervalo cerrado de parametros `t` sobre el que se busca interseccion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub min: f64,
    pub max: f64,
}

impl Interval {
    pub const EMPTY: Interval = Interval {
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
    };

    #[inline]
    pub const fn new(min: f64, max: f64) -> Interval {
        Interval { min, max }
    }

    /// Intervalo estandar para un rayo primario: desde justo delante del origen
    /// hasta el infinito.
    #[inline]
    pub const fn positive() -> Interval {
        Interval {
            min: 1e-9,
            max: f64::INFINITY,
        }
    }

    #[inline]
    pub fn contains(&self, t: f64) -> bool {
        self.min <= t && t <= self.max
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.min > self.max
    }

    #[inline]
    pub fn size(&self) -> f64 {
        (self.max - self.min).max(0.0)
    }

    /// Interseccion de dos intervalos.
    #[inline]
    pub fn intersect(&self, o: Interval) -> Interval {
        Interval::new(self.min.max(o.min), self.max.min(o.max))
    }
}

/// Medio en el que viaja el rayo: determina el indice de refraccion de partida y
/// cuanta energia se pierde por absorcion a lo largo del trayecto.
///
/// Propagar el medio por la recursion, en lugar de deducirlo de la cara
/// impactada, es lo que permite distinguir correctamente la entrada de la salida
/// de un volumen de agua o de vidrio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Medium {
    /// Indice de refraccion del medio actual.
    pub ior: f64,
    /// Coeficiente de absorcion por unidad de distancia (Beer-Lambert).
    pub absorption: Vec3,
}

impl Medium {
    /// El aire de la escena: no desvia y no absorbe.
    pub const AIR: Medium = Medium {
        ior: 1.0,
        absorption: v3(0.0, 0.0, 0.0),
    };

    #[inline]
    pub fn is_absorbing(&self) -> bool {
        self.absorption.max_component() > 1e-6
    }

    /// Transmitancia del medio a lo largo de una distancia, por Beer-Lambert.
    #[inline]
    pub fn transmittance(&self, distance: f64) -> Vec3 {
        if self.is_absorbing() {
            (self.absorption * distance).exp_neg()
        } else {
            Vec3::ONE
        }
    }
}

impl Default for Medium {
    fn default() -> Medium {
        Medium::AIR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_rayo_normaliza_y_precalcula_el_reciproco() {
        let r = Ray::new(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 4.0));
        assert!((r.dir.length() - 1.0).abs() < 1e-12);
        assert!((r.inv_dir.z - 1.0).abs() < 1e-12);
        assert!(r.inv_dir.x.is_infinite());
        assert!((r.at(3.0).z - 3.0).abs() < 1e-12);
    }

    #[test]
    fn el_desplazamiento_secundario_elige_el_lado_por_la_direccion() {
        let p = v3(1.0, 2.0, 3.0);
        let n = v3(0.0, 1.0, 0.0);
        // Reflexion: sale hacia el mismo lado que la normal.
        let arriba = offset_ray(p, n, v3(0.0, 1.0, 0.0));
        assert!(arriba.origin.y > p.y);
        // Refraccion: cruza la superficie, el origen debe quedar del otro lado.
        let abajo = offset_ray(p, n, v3(0.0, -1.0, 0.0));
        assert!(abajo.origin.y < p.y);
        assert!((abajo.origin.y - p.y).abs() <= SHADOW_EPSILON);
    }

    #[test]
    fn los_intervalos_se_intersectan_y_detectan_el_vacio() {
        let a = Interval::new(1.0, 5.0);
        assert!(a.contains(1.0) && a.contains(5.0) && !a.contains(5.1));
        assert_eq!(
            a.intersect(Interval::new(3.0, 9.0)),
            Interval::new(3.0, 5.0)
        );
        assert!(a.intersect(Interval::new(6.0, 9.0)).is_empty());
        assert!(Interval::EMPTY.is_empty());
        assert!((a.size() - 4.0).abs() < 1e-12);
    }

    #[test]
    fn el_aire_no_absorbe_y_el_agua_si() {
        assert!(!Medium::AIR.is_absorbing());
        assert_eq!(Medium::AIR.transmittance(100.0), Vec3::ONE);
        let agua = Medium {
            ior: 1.333,
            absorption: v3(0.35, 0.12, 0.08),
        };
        assert!(agua.is_absorbing());
        let t = agua.transmittance(3.0);
        assert!(t.x < t.y && t.y < t.z);
        assert!(t.x > 0.0);
    }
}
