//! Cubos alineados a los ejes: interseccion, caras, coordenadas UV y la base
//! tangente que necesitan los mapas normales.
//!
//! Toda la geometria visible del diorama son cubos unitarios colocados sobre una
//! retícula entera. Eso permite describir un impacto con el indice de la cara
//! cruzada, y de ahi derivar normal, UV, tangente y bitangente con una tabla
//! constante en lugar de con datos por vertice.

use crate::math::{v3, Vec3};
use crate::ray::{Interval, Ray};

/// Indice de cara: `+X`.
pub const FACE_POS_X: usize = 0;
/// Indice de cara: `-X`.
pub const FACE_NEG_X: usize = 1;
/// Indice de cara: `+Y` (la cara superior).
pub const FACE_POS_Y: usize = 2;
/// Indice de cara: `-Y`.
pub const FACE_NEG_Y: usize = 3;
/// Indice de cara: `+Z`.
pub const FACE_POS_Z: usize = 4;
/// Indice de cara: `-Z`.
pub const FACE_NEG_Z: usize = 5;
/// Sentinela: el rayo ya estaba dentro de la caja, no cruzo ninguna cara.
pub const FACE_INSIDE: usize = 6;

/// Normales salientes de las seis caras, indexadas por los constantes anteriores.
pub const FACE_NORMALS: [Vec3; 6] = [
    v3(1.0, 0.0, 0.0),
    v3(-1.0, 0.0, 0.0),
    v3(0.0, 1.0, 0.0),
    v3(0.0, -1.0, 0.0),
    v3(0.0, 0.0, 1.0),
    v3(0.0, 0.0, -1.0),
];

/// Cara correspondiente a un eje (`0 = x`, `1 = y`, `2 = z`) y un signo.
#[inline]
pub const fn face_of(axis: usize, positive: bool) -> usize {
    axis * 2 + if positive { 0 } else { 1 }
}

/// Eje al que pertenece una cara.
#[inline]
pub const fn face_axis(face: usize) -> usize {
    face / 2
}

/// Normal saliente de una cara.
#[inline]
pub fn face_normal(face: usize) -> Vec3 {
    FACE_NORMALS[face]
}

/// Base tangente de cada cara: `(tangente, bitangente)`.
///
/// La tabla se deriva de las UV de [`face_uv`]: la tangente es la derivada de la
/// posicion respecto de `u` y la bitangente respecto de `v`. Las seis ternas
/// cumplen `T x B = N`, es decir la base es derecha en todas las caras, que es
/// la condicion para que un mapa normal no aparezca invertido en unas caras y
/// correcto en otras.
pub const FACE_TANGENTS: [(Vec3, Vec3); 6] = [
    (v3(0.0, 0.0, 1.0), v3(0.0, -1.0, 0.0)),  // +X
    (v3(0.0, 0.0, -1.0), v3(0.0, -1.0, 0.0)), // -X
    (v3(1.0, 0.0, 0.0), v3(0.0, 0.0, -1.0)),  // +Y
    (v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)),   // -Y
    (v3(-1.0, 0.0, 0.0), v3(0.0, -1.0, 0.0)), // +Z
    (v3(1.0, 0.0, 0.0), v3(0.0, -1.0, 0.0)),  // -Z
];

/// Coordenadas UV de un punto sobre una cara del cubo.
///
/// `local` son las coordenadas del punto dentro de la celda, en `[0, 1]^3`,
/// medidas desde la esquina minima. `v` crece hacia abajo, igual que las filas
/// de una imagen, de modo que las texturas se leen sin voltear.
#[inline]
pub fn face_uv(face: usize, local: Vec3) -> (f64, f64) {
    let (lx, ly, lz) = (local.x, local.y, local.z);
    match face {
        FACE_POS_X => (lz, 1.0 - ly),
        FACE_NEG_X => (1.0 - lz, 1.0 - ly),
        FACE_POS_Y => (lx, 1.0 - lz),
        FACE_NEG_Y => (lx, lz),
        FACE_POS_Z => (1.0 - lx, 1.0 - ly),
        _ => (lx, 1.0 - ly),
    }
}

/// Caja alineada a los ejes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

/// Resultado del test de rebanadas: tramo del rayo dentro de la caja y las caras
/// por las que entra y sale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slab {
    pub t_enter: f64,
    pub t_exit: f64,
    /// Cara de entrada, o [`FACE_INSIDE`] si el rayo nacio dentro de la caja.
    pub enter_face: usize,
    pub exit_face: usize,
}

impl Aabb {
    #[inline]
    pub const fn new(min: Vec3, max: Vec3) -> Aabb {
        Aabb { min, max }
    }

    /// Caja de la celda unitaria situada en las coordenadas enteras dadas.
    #[inline]
    pub fn cell(i: i32, j: i32, k: i32) -> Aabb {
        let min = v3(i as f64, j as f64, k as f64);
        Aabb::new(min, min + Vec3::ONE)
    }

    #[inline]
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    #[inline]
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    /// Interseccion rayo-caja por el metodo de rebanadas, devolviendo las caras.
    ///
    /// Los rayos exactamente paralelos a un par de caras se resuelven con una
    /// comparacion explicita en lugar de dejar que `0 * infinito` produzca un
    /// `NaN`: es el caso que aparece constantemente al rasar muros y suelos.
    pub fn hit(&self, ray: &Ray, range: Interval) -> Option<Slab> {
        let mut t_enter = range.min;
        let mut t_exit = range.max;
        let mut enter_face = FACE_INSIDE;
        let mut exit_face = FACE_INSIDE;

        for axis in 0..3 {
            let d = ray.dir.axis(axis);
            let o = ray.origin.axis(axis);
            let lo = self.min.axis(axis);
            let hi = self.max.axis(axis);

            if d.abs() < 1e-18 {
                // Rayo paralelo a este par de caras: o esta dentro de la
                // rebanada para siempre, o no la toca nunca.
                if o < lo || o > hi {
                    return None;
                }
                continue;
            }

            let inv = ray.inv_dir.axis(axis);
            let mut t_near = (lo - o) * inv;
            let mut t_far = (hi - o) * inv;
            let mut near_face = face_of(axis, false);
            let mut far_face = face_of(axis, true);
            if t_near > t_far {
                std::mem::swap(&mut t_near, &mut t_far);
                std::mem::swap(&mut near_face, &mut far_face);
            }
            if t_near > t_enter {
                t_enter = t_near;
                enter_face = near_face;
            }
            if t_far < t_exit {
                t_exit = t_far;
                exit_face = far_face;
            }
            if t_exit <= t_enter {
                return None;
            }
        }

        if t_exit < range.min || t_enter > range.max {
            return None;
        }

        Some(Slab {
            t_enter,
            t_exit,
            enter_face,
            exit_face,
        })
    }
}

/// Datos completos de un impacto, tal como los consume el sombreado.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    /// Distancia a lo largo del rayo (la direccion esta normalizada).
    pub t: f64,
    pub point: Vec3,
    /// Normal geometrica saliente de la superficie, sin el mapa normal aplicado.
    pub normal: Vec3,
    /// Tangente de la cara, derivada de la posicion respecto de `u`.
    pub tangent: Vec3,
    /// Bitangente de la cara, derivada de la posicion respecto de `v`.
    pub bitangent: Vec3,
    pub u: f64,
    pub v: f64,
    /// Identificador del material de la superficie impactada.
    pub material: u16,
    /// Verdadero si el rayo llega desde el lado exterior de la superficie.
    pub front_face: bool,
    /// Cara del cubo que se cruzo.
    pub face: usize,
    /// Celda de la retícula a la que pertenece la superficie.
    pub cell: [i32; 3],
}

impl Hit {
    /// Construye el impacto a partir de la celda y la cara cruzadas.
    ///
    /// `outward` es la normal que apunta hacia fuera del material: al entrar en
    /// un bloque coincide con la normal de la cara cruzada, y al salir de un
    /// volumen de agua o vidrio es la opuesta. De ella se deduce `front_face`.
    pub fn from_face(
        t: f64,
        point: Vec3,
        cell: [i32; 3],
        face: usize,
        material: u16,
        ray_dir: Vec3,
    ) -> Hit {
        let local = v3(
            point.x - cell[0] as f64,
            point.y - cell[1] as f64,
            point.z - cell[2] as f64,
        );
        let (u, v) = face_uv(face, local);
        let (tangent, bitangent) = FACE_TANGENTS[face];
        let normal = face_normal(face);
        Hit {
            t,
            point,
            normal,
            tangent,
            bitangent,
            u,
            v,
            material,
            front_face: ray_dir.dot(normal) < 0.0,
            face,
            cell,
        }
    }

    /// Invierte la orientacion de la superficie manteniendo la base derecha.
    ///
    /// Se usa al abandonar un medio transparente: la cara cruzada es la de la
    /// celda de aire en la que entramos, pero la superficie pertenece al agua o
    /// al vidrio que dejamos atras, asi que su normal saliente es la opuesta.
    pub fn flipped(mut self) -> Hit {
        self.normal = -self.normal;
        self.bitangent = -self.bitangent;
        self.front_face = !self.front_face;
        self
    }

    /// Normal orientada siempre contra el rayo incidente.
    #[inline]
    pub fn facing_normal(&self) -> Vec3 {
        if self.front_face {
            self.normal
        } else {
            -self.normal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::degrees_to_radians;

    fn caja() -> Aabb {
        Aabb::new(v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0))
    }

    #[test]
    fn impacto_desde_fuera_entra_por_la_cara_correcta() {
        let r = Ray::new(v3(0.5, 0.5, -3.0), v3(0.0, 0.0, 1.0));
        let s = caja().hit(&r, Interval::positive()).expect("debe impactar");
        assert!((s.t_enter - 3.0).abs() < 1e-12);
        assert!((s.t_exit - 4.0).abs() < 1e-12);
        assert_eq!(s.enter_face, FACE_NEG_Z);
        assert_eq!(s.exit_face, FACE_POS_Z);
    }

    #[test]
    fn impacto_desde_dentro_solo_reporta_la_salida() {
        let r = Ray::new(v3(0.5, 0.5, 0.5), v3(1.0, 0.0, 0.0));
        let s = caja().hit(&r, Interval::positive()).expect("debe salir");
        assert_eq!(s.enter_face, FACE_INSIDE);
        assert_eq!(s.exit_face, FACE_POS_X);
        assert!((s.t_exit - 0.5).abs() < 1e-12);
    }

    #[test]
    fn rayo_paralelo_a_las_caras_fuera_de_la_rebanada_no_impacta() {
        // Paralelo al plano XZ, por encima de la caja.
        let r = Ray::new(v3(-3.0, 1.5, 0.5), v3(1.0, 0.0, 0.0));
        assert!(caja().hit(&r, Interval::positive()).is_none());
    }

    #[test]
    fn rayo_paralelo_a_las_caras_dentro_de_la_rebanada_si_impacta() {
        let r = Ray::new(v3(-3.0, 0.5, 0.5), v3(1.0, 0.0, 0.0));
        let s = caja().hit(&r, Interval::positive()).expect("debe impactar");
        assert_eq!(s.enter_face, FACE_NEG_X);
        assert!((s.t_enter - 3.0).abs() < 1e-12);
    }

    #[test]
    fn rayo_rasante_sobre_una_cara_no_produce_nan() {
        // Origen exactamente sobre el plano y = 1, direccion paralela a el.
        let r = Ray::new(v3(-3.0, 1.0, 0.5), v3(1.0, 0.0, 0.0));
        let s = caja().hit(&r, Interval::positive());
        assert!(s.is_some());
        let s = s.unwrap();
        assert!(s.t_enter.is_finite() && s.t_exit.is_finite());
    }

    #[test]
    fn el_rayo_que_se_aleja_no_impacta() {
        let r = Ray::new(v3(0.5, 0.5, -3.0), v3(0.0, 0.0, -1.0));
        assert!(caja().hit(&r, Interval::positive()).is_none());
    }

    #[test]
    fn el_intervalo_recorta_impactos_lejanos() {
        let r = Ray::new(v3(0.5, 0.5, -3.0), v3(0.0, 0.0, 1.0));
        assert!(caja().hit(&r, Interval::new(1e-9, 2.0)).is_none());
        assert!(caja().hit(&r, Interval::new(1e-9, 3.5)).is_some());
    }

    #[test]
    fn interseccion_oblicua_coincide_con_el_calculo_analitico() {
        let ang = degrees_to_radians(30.0);
        let dir = v3(ang.sin(), 0.0, ang.cos());
        let r = Ray::new(v3(0.2, 0.5, -1.0), dir);
        let s = caja().hit(&r, Interval::positive()).expect("debe impactar");
        // Entra por z = 0: t = 1 / cos(30).
        assert!((s.t_enter - 1.0 / ang.cos()).abs() < 1e-12);
        assert_eq!(s.enter_face, FACE_NEG_Z);
    }

    #[test]
    fn las_seis_bases_tangentes_son_derechas() {
        for (face, &(t, b)) in FACE_TANGENTS.iter().enumerate() {
            let n = face_normal(face);
            assert!((t.cross(b) - n).length() < 1e-12, "cara {face}");
            assert!(t.dot(b).abs() < 1e-12);
            assert!(t.dot(n).abs() < 1e-12);
            assert!(b.dot(n).abs() < 1e-12);
        }
    }

    #[test]
    fn la_tangente_es_la_derivada_de_la_posicion_respecto_de_u() {
        // Comprobacion numerica: mover el punto en la direccion de la tangente
        // debe aumentar u y dejar v igual.
        let h = 1e-4;
        for (face, &(t, b)) in FACE_TANGENTS.iter().enumerate() {
            let centro = v3(0.5, 0.5, 0.5);
            let (u0, v0) = face_uv(face, centro);
            let (u1, v1) = face_uv(face, centro + t * h);
            let (u2, v2) = face_uv(face, centro + b * h);
            assert!(u1 - u0 > h * 0.5, "cara {face}: u debe crecer con T");
            assert!(
                (v1 - v0).abs() < 1e-9,
                "cara {face}: v no debe cambiar con T"
            );
            assert!(v2 - v0 > h * 0.5, "cara {face}: v debe crecer con B");
            assert!(
                (u2 - u0).abs() < 1e-9,
                "cara {face}: u no debe cambiar con B"
            );
        }
    }

    #[test]
    fn las_uv_cubren_la_cara_completa_sin_salirse() {
        for face in 0..FACE_TANGENTS.len() {
            for &x in &[0.0, 0.5, 1.0] {
                for &y in &[0.0, 0.5, 1.0] {
                    for &z in &[0.0, 0.5, 1.0] {
                        let (u, v) = face_uv(face, v3(x, y, z));
                        assert!((0.0..=1.0).contains(&u), "cara {face} u={u}");
                        assert!((0.0..=1.0).contains(&v), "cara {face} v={v}");
                    }
                }
            }
        }
    }

    #[test]
    fn la_cara_superior_mira_hacia_arriba_y_su_v_recorre_z() {
        assert_eq!(face_normal(FACE_POS_Y), v3(0.0, 1.0, 0.0));
        let (u0, v0) = face_uv(FACE_POS_Y, v3(0.25, 1.0, 0.0));
        let (u1, v1) = face_uv(FACE_POS_Y, v3(0.25, 1.0, 1.0));
        assert!((u0 - 0.25).abs() < 1e-12 && (u1 - 0.25).abs() < 1e-12);
        assert!(v0 > v1);
    }

    #[test]
    fn el_impacto_deduce_orientacion_y_uv() {
        let p = v3(3.25, 4.75, 5.0);
        let h = Hit::from_face(2.0, p, [3, 4, 5], FACE_NEG_Z, 7, v3(0.0, 0.0, 1.0));
        assert!(h.front_face);
        assert_eq!(h.normal, v3(0.0, 0.0, -1.0));
        assert!((h.u - 0.25).abs() < 1e-12);
        assert!((h.v - 0.25).abs() < 1e-12);
        assert_eq!(h.material, 7);

        let volteado = h.flipped();
        assert!(!volteado.front_face);
        assert_eq!(volteado.normal, v3(0.0, 0.0, 1.0));
        // La base sigue siendo derecha tras voltear.
        assert!((volteado.tangent.cross(volteado.bitangent) - volteado.normal).length() < 1e-12);
    }

    #[test]
    fn la_normal_encarada_siempre_se_opone_al_rayo() {
        let dir = v3(0.3, -0.8, 0.5).normalized();
        let h = Hit::from_face(1.0, v3(0.5, 1.0, 0.5), [0, 0, 0], FACE_POS_Y, 1, dir);
        assert!(h.facing_normal().dot(dir) < 0.0);
        let h2 = Hit::from_face(1.0, v3(0.5, 1.0, 0.5), [0, 0, 0], FACE_NEG_Y, 1, dir);
        assert!(h2.facing_normal().dot(dir) < 0.0);
    }
}
