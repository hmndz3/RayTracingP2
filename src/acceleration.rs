//! Estructura de aceleracion: rejilla voxel densa recorrida con DDA.
//!
//! # Por que una rejilla y no una BVH
//!
//! Toda la geometria del diorama son cubos unitarios colocados sobre una retícula
//! entera. Para esa geometria la rejilla gana en los tres frentes que importan:
//!
//! - La celda que ocupa un punto se calcula con una parte entera, asi que la
//!   consulta es O(1) exacta y no hay descenso por un arbol.
//! - No hay coste de construccion ni heuristica de particion: la escena se
//!   escribe directamente en el vector de celdas.
//! - Las cajas no se solapan ni se dejan huecos, de modo que el recorrido de
//!   Amanatides y Woo visita las celdas en orden estricto de distancia y puede
//!   parar en la primera superficie. Una BVH sobre miles de cajas iguales
//!   degeneraria en muchos nodos con volumen vacio y obligaria a mantener una
//!   pila por rayo.
//!
//! La rejilla tambien resuelve un problema que no es de rendimiento: al recorrer
//! celda a celda se conoce el material a los dos lados de cada cara, asi que se
//! puede decidir que una cara entre dos celdas del mismo medio no es una
//! superficie. Eso es lo que evita las interfaces falsas dentro de un volumen de
//! agua o de vidrio, que producirian reflejos y refracciones inexistentes en cada
//! junta entre bloques contiguos.

use crate::geometry::{face_of, Aabb, Hit, FACE_INSIDE};
use crate::material::AIR;
use crate::math::{v3, Vec3};
use crate::ray::{Interval, Ray};

/// Rejilla densa de celdas unitarias con el identificador de material de cada una.
#[derive(Debug, Clone)]
pub struct VoxelGrid {
    dims: [i32; 3],
    cells: Vec<u16>,
    bounds: Aabb,
}

/// Que hacer despues de examinar un cruce durante el recorrido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Seguir recorriendo la rejilla.
    Continue,
    /// Detener el recorrido.
    Stop,
}

impl VoxelGrid {
    /// Rejilla vacia de las dimensiones dadas, con la esquina minima en el origen.
    pub fn new(nx: i32, ny: i32, nz: i32) -> VoxelGrid {
        assert!(nx > 0 && ny > 0 && nz > 0, "dimensiones invalidas");
        VoxelGrid {
            dims: [nx, ny, nz],
            cells: vec![AIR; (nx * ny * nz) as usize],
            bounds: Aabb::new(Vec3::ZERO, v3(nx as f64, ny as f64, nz as f64)),
        }
    }

    #[inline]
    pub fn dims(&self) -> [i32; 3] {
        self.dims
    }

    #[inline]
    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    #[inline]
    pub fn in_bounds(&self, i: i32, j: i32, k: i32) -> bool {
        i >= 0 && j >= 0 && k >= 0 && i < self.dims[0] && j < self.dims[1] && k < self.dims[2]
    }

    #[inline]
    fn index(&self, i: i32, j: i32, k: i32) -> usize {
        // Orden y-z-x: las celdas de una misma columna vertical quedan juntas, que
        // es el eje sobre el que mas se consulta al construir la escena.
        ((k * self.dims[1] + j) * self.dims[0] + i) as usize
    }

    /// Material de una celda. Fuera de la rejilla devuelve vacio, de modo que el
    /// exterior se comporta como aire infinito sin necesidad de casos especiales.
    #[inline]
    pub fn get(&self, i: i32, j: i32, k: i32) -> u16 {
        if self.in_bounds(i, j, k) {
            self.cells[self.index(i, j, k)]
        } else {
            AIR
        }
    }

    /// Escribe una celda. Las coordenadas fuera de la rejilla se ignoran, para que
    /// el constructor de la escena pueda dibujar formas que se salen del borde sin
    /// tener que recortarlas.
    #[inline]
    pub fn set(&mut self, i: i32, j: i32, k: i32, material: u16) {
        if self.in_bounds(i, j, k) {
            let idx = self.index(i, j, k);
            self.cells[idx] = material;
        }
    }

    /// Escribe una celda solo si esta vacia.
    #[inline]
    pub fn set_if_empty(&mut self, i: i32, j: i32, k: i32, material: u16) {
        if self.get(i, j, k) == AIR {
            self.set(i, j, k, material);
        }
    }

    /// Numero de celdas ocupadas.
    pub fn solid_count(&self) -> usize {
        self.cells.iter().filter(|&&m| m != AIR).count()
    }

    /// Numero total de celdas de la rejilla.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Recorre todas las celdas ocupadas.
    pub fn iter_solid(&self) -> impl Iterator<Item = ([i32; 3], u16)> + '_ {
        let [nx, ny, _] = self.dims;
        self.cells.iter().enumerate().filter_map(move |(idx, &m)| {
            if m == AIR {
                return None;
            }
            let i = idx as i32 % nx;
            let j = (idx as i32 / nx) % ny;
            let k = idx as i32 / (nx * ny);
            Some(([i, j, k], m))
        })
    }

    /// Recorre la rejilla llamando a `on_crossing` en cada cambio de material.
    ///
    /// `entry` es el material en el que viaja el rayo al empezar. Una cara entre
    /// dos celdas del mismo material no genera llamada: asi un volumen de agua o
    /// de vidrio formado por muchos bloques se comporta como un unico cuerpo.
    ///
    /// El recorrido es el de Amanatides y Woo: se mantiene, por cada eje, el
    /// parametro del siguiente plano de celda, y se avanza siempre por el eje cuyo
    /// plano esta mas cerca. Cada iteracion cuesta una comparacion y una suma.
    pub fn traverse<F>(&self, ray: &Ray, range: Interval, entry: u16, mut on_crossing: F)
    where
        F: FnMut(&Hit) -> Step,
    {
        // Recortar contra el volumen de la rejilla: fuera de el no hay nada que
        // visitar y el bucle no debe recorrer el vacio.
        let Some(slab) = self.bounds.hit(ray, range) else {
            return;
        };
        let t_inicio = slab.t_enter.max(range.min);
        let t_final = slab.t_exit.min(range.max);
        if t_inicio > t_final {
            return;
        }

        // Un empujon minimo hacia delante evita que un origen situado justo sobre
        // un plano de celda elija la celda equivocada.
        let p = ray.at(t_inicio + 1e-9);
        let mut cell = [
            (p.x.floor() as i32).clamp(0, self.dims[0] - 1),
            (p.y.floor() as i32).clamp(0, self.dims[1] - 1),
            (p.z.floor() as i32).clamp(0, self.dims[2] - 1),
        ];

        let mut step = [0i32; 3];
        let mut t_max = [f64::INFINITY; 3];
        let mut t_delta = [f64::INFINITY; 3];
        for a in 0..3 {
            let d = ray.dir.axis(a);
            if d > 0.0 {
                step[a] = 1;
                t_max[a] = ((cell[a] + 1) as f64 - ray.origin.axis(a)) * ray.inv_dir.axis(a);
                t_delta[a] = ray.inv_dir.axis(a);
            } else if d < 0.0 {
                step[a] = -1;
                t_max[a] = (cell[a] as f64 - ray.origin.axis(a)) * ray.inv_dir.axis(a);
                t_delta[a] = -ray.inv_dir.axis(a);
            }
        }

        let mut anterior = entry;
        let mut t_entrada = t_inicio;
        // Cara por la que se entro en la celda actual. Al empezar dentro de la
        // rejilla no se cruzo ninguna.
        let mut cara = if slab.enter_face == FACE_INSIDE {
            FACE_INSIDE
        } else {
            slab.enter_face
        };

        loop {
            let actual = self.get(cell[0], cell[1], cell[2]);

            if actual != anterior && cara != FACE_INSIDE && t_entrada >= range.min {
                let punto = ray.at(t_entrada);
                let hit = if actual != AIR {
                    // Se entra en un material: la normal de la cara cruzada apunta
                    // hacia el lado del que venimos, que es el exterior.
                    Hit::from_face(t_entrada, punto, cell, cara, actual, ray.dir)
                } else {
                    // Se abandona un material hacia el vacio: la superficie
                    // pertenece a la celda que se deja atras y su normal saliente
                    // es la opuesta a la cara cruzada.
                    let previa = [
                        cell[0] - step[0] * usize::from(face_axis_is(cara, 0)) as i32,
                        cell[1] - step[1] * usize::from(face_axis_is(cara, 1)) as i32,
                        cell[2] - step[2] * usize::from(face_axis_is(cara, 2)) as i32,
                    ];
                    Hit::from_face(t_entrada, punto, previa, cara, anterior, ray.dir).flipped()
                };
                if on_crossing(&hit) == Step::Stop {
                    return;
                }
            }
            anterior = actual;

            // Avanzar por el eje cuyo plano de celda esta mas cerca.
            let eje = if t_max[0] < t_max[1] {
                if t_max[0] < t_max[2] {
                    0
                } else {
                    2
                }
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            if step[eje] == 0 || t_max[eje] > t_final {
                // Si se abandona la rejilla estando dentro de un material, la
                // superficie de salida todavia tiene que informarse.
                if anterior != AIR && t_final <= range.max && t_final > range.min {
                    let punto = ray.at(t_final);
                    if let Some(f) = salida_face(&slab.exit_face) {
                        let hit = Hit::from_face(t_final, punto, cell, f, anterior, ray.dir);
                        on_crossing(&hit);
                    }
                }
                return;
            }

            t_entrada = t_max[eje];
            cell[eje] += step[eje];
            cara = face_of(eje, step[eje] < 0);
            t_max[eje] += t_delta[eje];

            if !self.in_bounds(cell[0], cell[1], cell[2]) {
                // Una ultima comparacion contra el aire exterior, para no perder la
                // cara de salida de un material pegado al borde de la rejilla.
                if anterior != AIR && t_entrada >= range.min && t_entrada <= range.max {
                    let punto = ray.at(t_entrada);
                    let previa = [
                        cell[0] - step[0] * usize::from(face_axis_is(cara, 0)) as i32,
                        cell[1] - step[1] * usize::from(face_axis_is(cara, 1)) as i32,
                        cell[2] - step[2] * usize::from(face_axis_is(cara, 2)) as i32,
                    ];
                    let hit =
                        Hit::from_face(t_entrada, punto, previa, cara, anterior, ray.dir).flipped();
                    on_crossing(&hit);
                }
                return;
            }
        }
    }

    /// Primera superficie que encuentra el rayo, o `None`.
    pub fn hit(&self, ray: &Ray, range: Interval, entry: u16) -> Option<Hit> {
        let mut resultado = None;
        self.traverse(ray, range, entry, |h| {
            resultado = Some(*h);
            Step::Stop
        });
        resultado
    }

    /// Busqueda exhaustiva, usada como referencia independiente en las pruebas.
    ///
    /// No comparte nada con el recorrido acelerado: enumera las caras entre celdas
    /// de material distinto y resuelve cada una como un plano recortado a un
    /// cuadrado, en lugar de avanzar por la rejilla.
    pub fn reference_hit(&self, ray: &Ray, range: Interval, _entry: u16) -> Option<Hit> {
        let [nx, ny, nz] = self.dims;
        let mut mejor: Option<Hit> = None;

        // Se recorre desde -1 para incluir tambien las caras contra el exterior.
        for k in -1..nz {
            for j in -1..ny {
                for i in -1..nx {
                    let aqui = self.get(i, j, k);
                    for eje in 0..3 {
                        let mut vecino = [i, j, k];
                        vecino[eje] += 1;
                        let alla = self.get(vecino[0], vecino[1], vecino[2]);
                        if aqui == alla {
                            continue;
                        }

                        // Plano perpendicular al eje, en la coordenada entera.
                        let plano = ([i, j, k][eje] + 1) as f64;
                        let d = ray.dir.axis(eje);
                        if d.abs() < 1e-18 {
                            continue;
                        }
                        let t = (plano - ray.origin.axis(eje)) * ray.inv_dir.axis(eje);
                        if !range.contains(t) {
                            continue;
                        }
                        let p = ray.at(t);

                        // Recortar al cuadrado de la cara.
                        let (a, b) = ((eje + 1) % 3, (eje + 2) % 3);
                        let dentro = |ax: usize, base: i32| {
                            let c = p.axis(ax);
                            c >= base as f64 - 1e-9 && c <= (base + 1) as f64 + 1e-9
                        };
                        if !dentro(a, [i, j, k][a]) || !dentro(b, [i, j, k][b]) {
                            continue;
                        }

                        // El material que se ve es aquel en el que se entra.
                        let entrando_a_alla = d > 0.0;
                        let (celda, mat, voltear) = if entrando_a_alla {
                            if alla != AIR {
                                (vecino, alla, false)
                            } else {
                                ([i, j, k], aqui, true)
                            }
                        } else if aqui != AIR {
                            ([i, j, k], aqui, false)
                        } else {
                            (vecino, alla, true)
                        };
                        // Una cara es superficie por los materiales que tiene a
                        // cada lado, no por el medio en el que viaja el rayo: si
                        // se descartase por coincidir con `entry` se perderia la
                        // cara de salida de un rayo nacido dentro de un material.
                        if mat == AIR {
                            continue;
                        }

                        let cara = face_of(eje, !entrando_a_alla);
                        let mut h = Hit::from_face(t, p, celda, cara, mat, ray.dir);
                        if voltear {
                            h = h.flipped();
                        }
                        if mejor.map_or(true, |m| t < m.t) {
                            mejor = Some(h);
                        }
                    }
                }
            }
        }
        mejor
    }
}

/// Verdadero si la cara pertenece al eje dado.
#[inline]
fn face_axis_is(face: usize, axis: usize) -> bool {
    face / 2 == axis
}

/// La cara de salida de la rejilla, si el rayo llego a cruzarla.
#[inline]
fn salida_face(face: &usize) -> Option<usize> {
    if *face == FACE_INSIDE {
        None
    } else {
        Some(*face)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::{STONE_ANCIENT, WATER};
    use crate::math::Rng;

    const PIEDRA: u16 = STONE_ANCIENT;
    const AGUA: u16 = WATER;
    const MADERA: u16 = crate::material::WOOD_AGED;

    fn rejilla_con_bloque() -> VoxelGrid {
        let mut g = VoxelGrid::new(8, 8, 8);
        g.set(3, 3, 3, PIEDRA);
        g
    }

    #[test]
    fn la_rejilla_empieza_vacia_y_acepta_escrituras() {
        let mut g = VoxelGrid::new(4, 5, 6);
        assert_eq!(g.cell_count(), 120);
        assert_eq!(g.solid_count(), 0);
        g.set(1, 2, 3, PIEDRA);
        assert_eq!(g.get(1, 2, 3), PIEDRA);
        assert_eq!(g.solid_count(), 1);
        // Fuera de la rejilla se comporta como aire y no falla.
        g.set(-1, 0, 0, PIEDRA);
        g.set(99, 0, 0, PIEDRA);
        assert_eq!(g.get(-1, 0, 0), AIR);
        assert_eq!(g.get(99, 99, 99), AIR);
        assert_eq!(g.solid_count(), 1);
    }

    #[test]
    fn set_if_empty_no_pisa_lo_ya_escrito() {
        let mut g = VoxelGrid::new(4, 4, 4);
        g.set(1, 1, 1, PIEDRA);
        g.set_if_empty(1, 1, 1, MADERA);
        assert_eq!(g.get(1, 1, 1), PIEDRA);
        g.set_if_empty(2, 1, 1, MADERA);
        assert_eq!(g.get(2, 1, 1), MADERA);
    }

    #[test]
    fn iter_solid_devuelve_las_coordenadas_correctas() {
        let mut g = VoxelGrid::new(6, 7, 8);
        let esperadas = [([0, 0, 0], PIEDRA), ([5, 6, 7], AGUA), ([2, 3, 4], MADERA)];
        for (c, m) in esperadas {
            g.set(c[0], c[1], c[2], m);
        }
        let mut vistas: Vec<_> = g.iter_solid().collect();
        vistas.sort_by_key(|(c, _)| (c[0], c[1], c[2]));
        let mut esperadas = esperadas.to_vec();
        esperadas.sort_by_key(|(c, _)| (c[0], c[1], c[2]));
        assert_eq!(vistas, esperadas);
    }

    #[test]
    fn impacto_frontal_contra_un_bloque_aislado() {
        let g = rejilla_con_bloque();
        let r = Ray::new(v3(3.5, 3.5, 0.0), v3(0.0, 0.0, 1.0));
        let h = g.hit(&r, Interval::positive(), AIR).expect("debe impactar");
        assert_eq!(h.material, PIEDRA);
        assert!((h.t - 3.0).abs() < 1e-9);
        assert_eq!(h.normal, v3(0.0, 0.0, -1.0));
        assert!(h.front_face);
        assert_eq!(h.cell, [3, 3, 3]);
    }

    #[test]
    fn el_rayo_que_no_toca_nada_no_impacta() {
        let g = rejilla_con_bloque();
        let r = Ray::new(v3(0.5, 0.5, 0.0), v3(0.0, 0.0, 1.0));
        assert!(g.hit(&r, Interval::positive(), AIR).is_none());
        // Y tampoco si el rayo pasa completamente fuera de la rejilla.
        let fuera = Ray::new(v3(-5.0, 3.5, 3.5), v3(0.0, 0.0, 1.0));
        assert!(g.hit(&fuera, Interval::positive(), AIR).is_none());
    }

    #[test]
    fn se_impacta_desde_dentro_del_bloque() {
        let g = rejilla_con_bloque();
        // Origen en el centro del cubo: la primera superficie es su cara de salida.
        let r = Ray::new(v3(3.5, 3.5, 3.5), v3(1.0, 0.0, 0.0));
        let h = g.hit(&r, Interval::positive(), PIEDRA).expect("debe salir");
        assert_eq!(h.material, PIEDRA);
        assert!((h.t - 0.5).abs() < 1e-9);
        assert!(!h.front_face, "se sale del material");
        assert_eq!(
            h.normal,
            v3(1.0, 0.0, 0.0),
            "la normal saliente mira hacia +X"
        );
    }

    #[test]
    fn un_rayo_paralelo_a_las_caras_recorre_sin_impactar() {
        let mut g = VoxelGrid::new(8, 8, 8);
        for i in 0..8 {
            g.set(i, 2, 4, PIEDRA);
        }
        // Justo por encima de la fila, rozandola sin tocarla.
        let r = Ray::new(v3(-1.0, 3.5, 4.5), v3(1.0, 0.0, 0.0));
        assert!(g.hit(&r, Interval::positive(), AIR).is_none());
        // Y dentro de la fila si impacta.
        let r2 = Ray::new(v3(-1.0, 2.5, 4.5), v3(1.0, 0.0, 0.0));
        let h = g.hit(&r2, Interval::positive(), AIR).unwrap();
        assert!((h.t - 1.0).abs() < 1e-9);
    }

    #[test]
    fn un_rayo_exactamente_sobre_el_plano_de_una_cara_no_se_pierde() {
        let mut g = VoxelGrid::new(8, 8, 8);
        for i in 0..8 {
            for k in 0..8 {
                g.set(i, 2, k, PIEDRA);
            }
        }
        // Origen exactamente sobre la cara superior del suelo.
        let r = Ray::new(v3(0.5, 3.0, 0.5), v3(0.3, -0.2, 0.9));
        let h = g.hit(&r, Interval::positive(), AIR);
        assert!(h.is_some(), "deberia encontrar el suelo");
        assert!(h.unwrap().t.is_finite());
    }

    #[test]
    fn las_celdas_contiguas_del_mismo_medio_no_generan_interfaz() {
        // Un volumen de agua de tres bloques debe comportarse como un solo cuerpo:
        // exactamente dos superficies, la de entrada y la de salida.
        let mut g = VoxelGrid::new(8, 8, 8);
        for i in 2..5 {
            g.set(i, 3, 3, AGUA);
        }
        let r = Ray::new(v3(0.0, 3.5, 3.5), v3(1.0, 0.0, 0.0));
        let mut cruces = Vec::new();
        g.traverse(&r, Interval::positive(), AIR, |h| {
            cruces.push((h.t, h.material, h.front_face));
            Step::Continue
        });
        assert_eq!(
            cruces.len(),
            2,
            "interfaces falsas entre bloques: {cruces:?}"
        );
        assert!((cruces[0].0 - 2.0).abs() < 1e-9 && cruces[0].2);
        assert!((cruces[1].0 - 5.0).abs() < 1e-9 && !cruces[1].2);
        assert_eq!(cruces[0].1, AGUA);
        assert_eq!(cruces[1].1, AGUA);
    }

    #[test]
    fn dos_medios_distintos_contiguos_si_generan_interfaz() {
        let mut g = VoxelGrid::new(8, 8, 8);
        g.set(2, 3, 3, AGUA);
        g.set(3, 3, 3, PIEDRA);
        let r = Ray::new(v3(0.0, 3.5, 3.5), v3(1.0, 0.0, 0.0));
        let mut cruces = Vec::new();
        g.traverse(&r, Interval::positive(), AIR, |h| {
            cruces.push((h.t, h.material));
            Step::Continue
        });
        assert_eq!(cruces.len(), 3);
        assert_eq!(cruces[0].1, AGUA);
        assert_eq!(cruces[1].1, PIEDRA, "el agua da paso a la piedra en x = 3");
        assert!((cruces[1].0 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn un_rayo_que_nace_dentro_del_agua_sale_por_la_superficie() {
        let mut g = VoxelGrid::new(8, 8, 8);
        for i in 2..6 {
            for j in 2..4 {
                g.set(i, j, 3, AGUA);
            }
        }
        // Desde dentro del agua hacia arriba: solo debe encontrar la superficie.
        let r = Ray::new(v3(3.5, 2.5, 3.5), v3(0.0, 1.0, 0.0));
        let h = g.hit(&r, Interval::positive(), AGUA).expect("debe salir");
        assert_eq!(h.material, AGUA);
        assert!(!h.front_face);
        assert!((h.t - 1.5).abs() < 1e-9);
        assert_eq!(h.normal, v3(0.0, 1.0, 0.0));
    }

    #[test]
    fn se_detecta_la_salida_de_un_material_pegado_al_borde_de_la_rejilla() {
        let mut g = VoxelGrid::new(4, 4, 4);
        g.set(3, 2, 2, AGUA);
        let r = Ray::new(v3(3.5, 2.5, 2.5), v3(1.0, 0.0, 0.0));
        let h = g.hit(&r, Interval::positive(), AGUA).expect("debe salir");
        assert!(!h.front_face);
        assert!((h.t - 0.5).abs() < 1e-9);
    }

    #[test]
    fn el_intervalo_limita_el_recorrido() {
        let g = rejilla_con_bloque();
        let r = Ray::new(v3(3.5, 3.5, 0.0), v3(0.0, 0.0, 1.0));
        assert!(g.hit(&r, Interval::new(1e-9, 2.0), AIR).is_none());
        assert!(g.hit(&r, Interval::new(1e-9, 3.5), AIR).is_some());
        // Y tambien por delante: un impacto anterior al minimo se ignora.
        assert!(g.hit(&r, Interval::new(3.5, 100.0), AIR).is_some());
        assert_eq!(
            g.hit(&r, Interval::new(3.5, 100.0), AIR).unwrap().t,
            4.0,
            "deberia saltar a la cara de salida"
        );
    }

    /// Escena de prueba con relleno variado: suelo, columnas, un volumen de agua
    /// y bloques sueltos, de modo que la comparacion contra la referencia cubra
    /// materiales contiguos iguales y distintos.
    fn escena_variada(semilla: u64) -> VoxelGrid {
        let mut g = VoxelGrid::new(12, 10, 12);
        let mut rng = Rng::new(semilla);
        for k in 0..12 {
            for i in 0..12 {
                let h = 2 + (rng.next_u64() % 3) as i32;
                for j in 0..h {
                    g.set(i, j, k, if j == h - 1 { MADERA } else { PIEDRA });
                }
            }
        }
        // Estanque: un hueco relleno de agua.
        for k in 4..8 {
            for i in 3..7 {
                for j in 1..4 {
                    g.set(i, j, k, AGUA);
                }
            }
        }
        // Columnas y bloques sueltos.
        for _ in 0..40 {
            let i = (rng.next_u64() % 12) as i32;
            let k = (rng.next_u64() % 12) as i32;
            let j = 4 + (rng.next_u64() % 5) as i32;
            g.set(i, j, k, PIEDRA);
        }
        g
    }

    #[test]
    fn el_recorrido_acelerado_coincide_con_la_busqueda_exhaustiva() {
        let g = escena_variada(20260925);
        let mut rng = Rng::new(7);
        let mut comprobados = 0;
        let mut impactos = 0;

        for _ in 0..4000 {
            // Origen fuera de la rejilla, apuntando hacia dentro, en cualquier
            // direccion: cubre entradas por las seis caras.
            let origen = v3(6.0, 5.0, 6.0) + rng.unit_vector() * 18.0;
            let objetivo = v3(
                rng.range(0.0, 12.0),
                rng.range(0.0, 10.0),
                rng.range(0.0, 12.0),
            );
            let r = Ray::new(origen, objetivo - origen);

            let rapido = g.hit(&r, Interval::positive(), AIR);
            let lento = g.reference_hit(&r, Interval::positive(), AIR);
            comprobados += 1;

            match (rapido, lento) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    impactos += 1;
                    assert!(
                        (a.t - b.t).abs() < 1e-6,
                        "distancias distintas: {} vs {}",
                        a.t,
                        b.t
                    );
                    assert_eq!(a.material, b.material, "material distinto en t={}", a.t);
                    assert_eq!(a.face, b.face, "cara distinta en t={}", a.t);
                    assert_eq!(a.front_face, b.front_face);
                    assert!((a.u - b.u).abs() < 1e-6 && (a.v - b.v).abs() < 1e-6);
                }
                (a, b) => panic!("discrepancia: acelerado {a:?}, exhaustivo {b:?}"),
            }
        }
        assert_eq!(comprobados, 4000);
        assert!(impactos > 2000, "muy pocos impactos para ser concluyente");
    }

    #[test]
    fn el_recorrido_coincide_tambien_para_rayos_que_nacen_dentro() {
        let g = escena_variada(31415);
        let mut rng = Rng::new(99);
        let mut comparados = 0;
        for _ in 0..3000 {
            let origen = v3(
                rng.range(0.5, 11.5),
                rng.range(0.5, 9.5),
                rng.range(0.5, 11.5),
            );
            let celda = [
                origen.x.floor() as i32,
                origen.y.floor() as i32,
                origen.z.floor() as i32,
            ];
            let entrada = g.get(celda[0], celda[1], celda[2]);
            let r = Ray::new(origen, rng.unit_vector());

            let rapido = g.hit(&r, Interval::positive(), entrada);
            let lento = g.reference_hit(&r, Interval::positive(), entrada);
            match (rapido, lento) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    comparados += 1;
                    assert!((a.t - b.t).abs() < 1e-6, "{} vs {}", a.t, b.t);
                    assert_eq!(a.material, b.material);
                    assert_eq!(a.front_face, b.front_face);
                }
                (a, b) => panic!("discrepancia interior: {a:?} / {b:?}"),
            }
        }
        assert!(comparados > 1500);
    }

    #[test]
    fn el_recorrido_visita_las_celdas_en_orden_de_distancia() {
        let g = escena_variada(2718);
        let mut rng = Rng::new(555);
        for _ in 0..500 {
            let origen = v3(6.0, 5.0, 6.0) + rng.unit_vector() * 16.0;
            let r = Ray::new(origen, v3(6.0, 5.0, 6.0) - origen);
            let mut anterior = f64::NEG_INFINITY;
            g.traverse(&r, Interval::positive(), AIR, |h| {
                assert!(
                    h.t >= anterior - 1e-9,
                    "orden roto: {} tras {}",
                    h.t,
                    anterior
                );
                anterior = h.t;
                Step::Continue
            });
        }
    }

    #[test]
    fn detener_el_recorrido_no_visita_mas_celdas() {
        let g = escena_variada(161803);
        let r = Ray::new(v3(-8.0, 5.5, 6.5), v3(1.0, 0.0, 0.0));
        let mut cuenta = 0;
        g.traverse(&r, Interval::positive(), AIR, |_| {
            cuenta += 1;
            Step::Stop
        });
        assert_eq!(cuenta, 1);
    }

    #[test]
    fn ningun_impacto_produce_valores_no_finitos() {
        let g = escena_variada(11);
        let mut rng = Rng::new(1234);
        for _ in 0..3000 {
            let origen = v3(6.0, 5.0, 6.0) + rng.unit_vector() * rng.range(0.1, 25.0);
            let r = Ray::new(origen, rng.unit_vector());
            g.traverse(&r, Interval::positive(), AIR, |h| {
                assert!(h.t.is_finite() && h.t >= 0.0);
                assert!(h.point.is_finite());
                assert!((h.normal.length() - 1.0).abs() < 1e-9);
                assert!((0.0..=1.0).contains(&h.u) && (0.0..=1.0).contains(&h.v));
                Step::Continue
            });
        }
    }
}
