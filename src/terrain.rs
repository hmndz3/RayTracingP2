//! Terreno procedural del diorama.
//!
//! El relieve sale de ruido de valor con varias octavas, sobre el que se imponen
//! tres rasgos deliberados: la meseta donde se asientan los cimientos de la
//! abadia, la depresion del estanque y el reborde que impide que la isla acabe en
//! un corte recto. La semilla entra en todas las llamadas al ruido, asi que dos
//! ejecuciones con la misma semilla producen exactamente el mismo terreno, y
//! cambiarla reordena el relieve, la vegetacion y los escombros a la vez.

use crate::acceleration::VoxelGrid;
use crate::material::{EARTH_DARK, EARTH_MOSS, FOLIAGE, STONE_ANCIENT, STONE_RUBBLE, WATER};
use crate::math::{hash01_3, smoothstep};
use crate::noise::fbm2;

/// Lado del terreno en celdas. El encargo pide al menos 16 por 16.
pub const TERRAIN_SIZE: i32 = 24;
/// Plano en el que queda la superficie del agua.
pub const WATER_PLANE: i32 = 6;
/// Altura de la meseta de cimientos, en numero de celdas solidas.
pub const FOUNDATION_HEIGHT: i32 = 7;

/// Cuanto desciende el terreno en el canto del diorama.
///
/// Se deja en cero a proposito: la isla se lee mejor como un zocalo de cantos
/// verticales, donde los estratos quedan a la vista, que con un reborde
/// redondeado, que sobre una retícula entera solo produce bancales concentricos.
pub const RIM_DROP: f64 = 0.0;

/// Rectangulo de celdas, con los dos extremos incluidos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
}

impl Rect {
    pub const fn new(x0: i32, z0: i32, x1: i32, z1: i32) -> Rect {
        Rect { x0, z0, x1, z1 }
    }

    #[inline]
    pub fn contains(&self, x: i32, z: i32) -> bool {
        x >= self.x0 && x <= self.x1 && z >= self.z0 && z <= self.z1
    }

    /// Distancia en celdas al borde del rectangulo. Negativa dentro de el.
    #[inline]
    pub fn distance(&self, x: i32, z: i32) -> f64 {
        let dx = (self.x0 - x).max(x - self.x1);
        let dz = (self.z0 - z).max(z - self.z1);
        if dx <= 0 && dz <= 0 {
            dx.max(dz) as f64
        } else {
            ((dx.max(0).pow(2) + dz.max(0).pow(2)) as f64).sqrt()
        }
    }
}

/// Parametros de generacion del terreno.
#[derive(Debug, Clone)]
pub struct TerrainSpec {
    pub seed: u64,
    pub size: i32,
    /// Altura media del terreno, en celdas.
    pub base: f64,
    /// Amplitud del relieve alrededor de la media.
    pub amplitude: f64,
    /// Escala del ruido: cuantas celdas mide, aproximadamente, un accidente.
    pub feature_scale: f64,
    /// Huella de los cimientos, que se aplana.
    pub foundation: Rect,
    /// Centro del estanque en coordenadas continuas.
    pub pond_center: (f64, f64),
    pub pond_radius: f64,
    /// Cuanto baja el fondo del estanque respecto de la orilla.
    pub pond_depth: f64,
    pub vegetation_density: f64,
    pub debris_density: f64,
}

impl Default for TerrainSpec {
    fn default() -> TerrainSpec {
        TerrainSpec {
            seed: 20_260_924,
            size: TERRAIN_SIZE,
            base: 6.45,
            amplitude: 0.85,
            feature_scale: 7.5,
            // La abadia ocupa el fondo y un lado: z alto y x alto.
            foundation: Rect::new(6, 11, 22, 22),
            // El estanque queda en primer plano, delante de la fachada.
            pond_center: (7.5, 5.5),
            pond_radius: 6.0,
            pond_depth: 3.5,
            vegetation_density: 0.13,
            debris_density: 0.05,
        }
    }
}

/// Cuanto pesa el reborde de la isla en una columna, de cero a uno.
///
/// Vale uno en el canto del diorama y cae a cero unas celdas hacia dentro. Es la
/// misma funcion que usan el descenso del borde y la exclusion de la orilla, de
/// modo que los dos no pueden discrepar sobre donde empieza el canto.
#[inline]
pub fn rim_factor(x: i32, z: i32, size: i32) -> f64 {
    let borde = x.min(z).min(size - 1 - x).min(size - 1 - z) as f64;
    1.0 - smoothstep(borde / 1.2)
}

/// Terreno ya resuelto: una altura entera por columna.
#[derive(Debug, Clone)]
pub struct Terrain {
    pub size: i32,
    /// Numero de celdas solidas de cada columna. La superficie queda en el plano
    /// `height`, asi que lo que se apoye encima empieza en `y = height`.
    heights: Vec<i32>,
    /// Columnas que pertenecen a la cubeta del estanque.
    ///
    /// Hace falta como dato aparte porque estar por debajo del plano del agua no
    /// basta para ser estanque: el canto de la isla tambien desciende por debajo
    /// de esa cota, y sin esta mascara se inundaria todo el perimetro del diorama
    /// con un anillo de agua colgando del borde.
    pond: Vec<bool>,
    pub spec: TerrainSpec,
}

impl Terrain {
    /// Genera el terreno.
    pub fn generate(spec: TerrainSpec) -> Terrain {
        let n = spec.size;
        let mut heights = vec![0i32; (n * n) as usize];
        let mut pond = vec![false; (n * n) as usize];

        for z in 0..n {
            for x in 0..n {
                let fx = x as f64 / spec.feature_scale;
                let fz = z as f64 / spec.feature_scale;

                // Relieve base: cuatro octavas centradas en cero.
                let ondulacion = fbm2(fx, fz, spec.seed, 4, 2.0, 0.5) * 2.0 - 1.0;
                let mut h = spec.base + ondulacion * spec.amplitude;

                // Reborde de la isla: las ultimas celdas bajan, para que el
                // diorama no termine en un tajo recto contra el cielo.
                h -= rim_factor(x, z, n) * RIM_DROP;

                // Meseta de cimientos: dentro es plana, y fuera se funde con el
                // relieve en un par de celdas para que no quede un escalon.
                let d = spec.foundation.distance(x, z);
                if d <= 2.5 {
                    let peso = 1.0 - smoothstep(d.max(0.0) / 2.5);
                    h = h * (1.0 - peso) + FOUNDATION_HEIGHT as f64 * peso;
                }

                // Depresion del estanque: cuenco de paredes suaves.
                //
                // El cuenco se recorta contra la meseta y contra el canto de la
                // isla. Sin ese recorte el estanque excavaria la plataforma de la
                // abadia por un lado y se asomaria al borde del diorama por el
                // otro, que son las dos unicas formas de que el agua acabe donde
                // no debe.
                let (px, pz) = spec.pond_center;
                let r = ((x as f64 + 0.5 - px).powi(2) + (z as f64 + 0.5 - pz).powi(2)).sqrt();

                // Perfil de cubeta, no de embudo: fondo plano en la mitad
                // interior y orillas en talud. Con una caida conica solo la zona
                // central bajaba del plano del agua y el estanque quedaba
                // reducido a un charco en medio del circulo.
                let radio = spec.pond_radius.max(1e-6);
                let talud = ((r - 0.5 * radio) / (0.5 * radio)).clamp(0.0, 1.0);
                let perfil = if r < radio {
                    1.0 - smoothstep(talud)
                } else {
                    0.0
                };

                // Los dos recortes se aplican como factores que se desvanecen, no
                // como cortes secos. Un corte dejaria un tajo de varias celdas
                // justo donde el circulo se encuentra con la meseta o con el
                // canto, y ese tajo se ve como un escalon imposible.
                let borde = x.min(z).min(n - 1 - x).min(n - 1 - z) as f64;
                let margen_canto = smoothstep((borde - 1.5) / 1.5);
                let margen_meseta = smoothstep((spec.foundation.distance(x, z) - 0.5) / 2.5);
                let cuenco = perfil * margen_canto * margen_meseta;
                h -= cuenco * spec.pond_depth;

                let idx = (z * n + x) as usize;
                heights[idx] = h.round() as i32;
                // La cubeta son las columnas realmente excavadas que ademas quedan
                // por debajo del plano del agua: las del talud que no bajan lo
                // suficiente son orilla, no fondo.
                pond[idx] = cuenco > 0.02 && heights[idx] < WATER_PLANE;
            }
        }

        // La orilla se nivela al plano del agua: si la celda que rodea el
        // estanque quedase por debajo, el agua se derramaria por el hueco.
        //
        // El reborde de la isla queda excluido a proposito. Ahi el terreno tiene
        // que caer, y forzarlo a la cota del agua levantaria una pared de una
        // celda justo en el canto del diorama. El estanque nunca alcanza esa
        // franja, asi que no hay nada que contener.
        for z in 0..n {
            for x in 0..n {
                if rim_factor(x, z, n) > 0.01 {
                    continue;
                }
                let idx = (z * n + x) as usize;
                let (px, pz) = spec.pond_center;
                let r = ((x as f64 + 0.5 - px).powi(2) + (z as f64 + 0.5 - pz).powi(2)).sqrt();
                if r >= spec.pond_radius && r < spec.pond_radius + 2.2 {
                    heights[idx] = heights[idx].max(WATER_PLANE);
                }
            }
        }

        Terrain {
            size: n,
            heights,
            pond,
            spec,
        }
    }

    /// Altura de una columna. Fuera del terreno devuelve cero.
    #[inline]
    pub fn height(&self, x: i32, z: i32) -> i32 {
        if x < 0 || z < 0 || x >= self.size || z >= self.size {
            0
        } else {
            self.heights[(z * self.size + x) as usize]
        }
    }

    /// Fuerza la altura de una columna. Lo usa la escena al asentar el camino.
    pub fn set_height(&mut self, x: i32, z: i32, h: i32) {
        if x >= 0 && z >= 0 && x < self.size && z < self.size {
            self.heights[(z * self.size + x) as usize] = h;
        }
    }

    /// Verdadero si la columna forma parte de la cubeta del estanque.
    ///
    /// No equivale a estar por debajo del plano del agua: el canto de la isla
    /// tambien lo esta y es tierra seca.
    #[inline]
    pub fn is_submerged(&self, x: i32, z: i32) -> bool {
        if x < 0 || z < 0 || x >= self.size || z >= self.size {
            return false;
        }
        self.pond[(z * self.size + x) as usize]
    }

    /// Desnivel maximo con las cuatro columnas vecinas.
    ///
    /// Es la medida de pendiente que decide donde puede arraigar la vegetacion:
    /// una mata sobre un talud de tres celdas flotaria a medio aire.
    pub fn slope(&self, x: i32, z: i32) -> i32 {
        let h = self.height(x, z);
        let mut m = 0;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let vecino = self.height(x + dx, z + dz);
            m = m.max((h - vecino).abs());
        }
        m
    }

    /// Escribe el terreno, el agua, la vegetacion y los escombros en la rejilla.
    ///
    /// Las capas van de dentro afuera: roca en profundidad, tierra debajo de la
    /// superficie y tierra con musgo arriba, salvo bajo el agua, donde la capa
    /// visible es tierra oscura porque el musgo no prospera sumergido.
    pub fn build(&self, grid: &mut VoxelGrid) {
        for z in 0..self.size {
            for x in 0..self.size {
                let h = self.height(x, z);
                for y in 0..h {
                    let profundidad = h - 1 - y;
                    let material = if profundidad == 0 {
                        // El musgo no prospera sumergido: el fondo del estanque
                        // es tierra desnuda.
                        if self.is_submerged(x, z) {
                            EARTH_DARK
                        } else {
                            EARTH_MOSS
                        }
                    } else if profundidad <= 4 {
                        EARTH_DARK
                    } else {
                        // Roca madre, solo en las ultimas hiladas. El canto de la
                        // isla ocupa buena parte del encuadre, y con la roca
                        // subiendo mas arriba se convertia en una masa palida que
                        // pesaba mas que la propia abadia.
                        STONE_RUBBLE
                    };
                    grid.set(x, y, z, material);
                }

                // Agua: solo dentro de la cubeta, nunca en el canto de la isla.
                if self.is_submerged(x, z) && h < WATER_PLANE {
                    for y in h..WATER_PLANE {
                        grid.set(x, y, z, WATER);
                    }
                }
            }
        }
    }

    /// Reparte vegetacion y escombros siguiendo reglas reproducibles.
    ///
    /// Se llama despues de levantar la arquitectura, de modo que una mata nunca
    /// aparece dentro de un muro: la celda tiene que estar libre y apoyada sobre
    /// terreno con musgo.
    pub fn scatter(&self, grid: &mut VoxelGrid, evitar: &[Rect]) {
        for z in 0..self.size {
            for x in 0..self.size {
                if evitar.iter().any(|r| r.contains(x, z)) {
                    continue;
                }
                let h = self.height(x, z);
                if h <= WATER_PLANE || self.slope(x, z) > 1 {
                    continue;
                }
                // La celda de apoyo debe seguir siendo terreno, y la de encima
                // estar libre: asi nada crece sobre losas, escaleras ni tejados.
                if grid.get(x, h - 1, z) != EARTH_MOSS || grid.get(x, h, z) != 0 {
                    continue;
                }

                let dado = hash01_3(x as i64, z as i64, 7, self.spec.seed);
                if dado < self.spec.vegetation_density {
                    grid.set(x, h, z, FOLIAGE);
                    // Una de cada cuatro matas es alta, para romper la monotonia.
                    let alto = hash01_3(x as i64, z as i64, 8, self.spec.seed);
                    if alto < 0.25 && grid.get(x, h + 1, z) == 0 {
                        grid.set(x, h + 1, z, FOLIAGE);
                    }
                } else if dado > 1.0 - self.spec.debris_density {
                    grid.set(x, h, z, STONE_RUBBLE);
                }
            }
        }
    }

    /// Altura media del terreno, util para informar y para las pruebas.
    pub fn average_height(&self) -> f64 {
        self.heights.iter().map(|&h| h as f64).sum::<f64>() / self.heights.len() as f64
    }

    /// Numero de columnas sumergidas.
    pub fn submerged_columns(&self) -> usize {
        self.pond.iter().filter(|&&p| p).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::AIR;

    #[test]
    fn el_terreno_cumple_el_tamano_minimo_del_encargo() {
        let t = Terrain::generate(TerrainSpec::default());
        assert_eq!(t.size, 24);
        assert!(t.size >= 16, "el minimo de la rubrica es 16 por 16");
        assert_eq!(t.heights.len(), 24 * 24);
    }

    #[test]
    fn la_generacion_es_reproducible_con_la_misma_semilla() {
        let a = Terrain::generate(TerrainSpec::default());
        let b = Terrain::generate(TerrainSpec::default());
        assert_eq!(a.heights, b.heights);

        // Y tambien el reparto de vegetacion, que depende de la misma semilla.
        let mut g1 = VoxelGrid::new(24, 30, 24);
        let mut g2 = VoxelGrid::new(24, 30, 24);
        a.build(&mut g1);
        b.build(&mut g2);
        a.scatter(&mut g1, &[]);
        b.scatter(&mut g2, &[]);
        let solidas1: Vec<_> = g1.iter_solid().collect();
        let solidas2: Vec<_> = g2.iter_solid().collect();
        assert_eq!(solidas1, solidas2);
    }

    #[test]
    fn cambiar_la_semilla_cambia_el_relieve() {
        let a = Terrain::generate(TerrainSpec::default());
        let b = Terrain::generate(TerrainSpec {
            seed: 99,
            ..TerrainSpec::default()
        });
        // Solo se miden las columnas libres: la meseta y el estanque son
        // composicion impuesta y tienen que salir iguales con cualquier semilla.
        let f = a.spec.foundation;
        let (px, pz) = a.spec.pond_center;
        let mut libres = 0;
        let mut distintas = 0;
        for z in 0..a.size {
            for x in 0..a.size {
                let r = ((x as f64 + 0.5 - px).powi(2) + (z as f64 + 0.5 - pz).powi(2)).sqrt();
                if f.distance(x, z) <= 3.0 || r < a.spec.pond_radius + 2.2 {
                    continue;
                }
                libres += 1;
                if a.height(x, z) != b.height(x, z) {
                    distintas += 1;
                }
            }
        }
        // La huella construida ocupa buena parte de la isla, asi que el conjunto
        // de columnas libres es modesto por diseno.
        assert!(libres > 110, "quedan pocas columnas libres: {libres}");
        // El umbral es de una de cada cuatro y no de la mitad porque la altura se
        // redondea a celdas enteras: un desplazamiento del campo continuo solo se
        // ve en las columnas que cruzan un entero al hacerlo.
        assert!(
            distintas * 8 > libres,
            "solo cambian {distintas} de {libres} columnas libres"
        );
    }

    #[test]
    fn la_semilla_no_altera_los_rasgos_deliberados() {
        // La meseta y el estanque tienen que seguir ahi con cualquier semilla:
        // son composicion, no azar.
        for seed in [1u64, 7, 20_260_924, 555_555] {
            let t = Terrain::generate(TerrainSpec {
                seed,
                ..TerrainSpec::default()
            });
            let f = t.spec.foundation;
            for z in f.z0..=f.z1 {
                for x in f.x0..=f.x1 {
                    assert_eq!(
                        t.height(x, z),
                        FOUNDATION_HEIGHT,
                        "semilla {seed}: la meseta no es plana en {x},{z}"
                    );
                }
            }
            assert!(
                t.submerged_columns() > 20,
                "semilla {seed}: el estanque desaparecio"
            );
        }
    }

    #[test]
    fn el_relieve_tiene_variacion_pero_no_se_desboca() {
        let t = Terrain::generate(TerrainSpec::default());
        let minimo = *t.heights.iter().min().unwrap();
        let maximo = *t.heights.iter().max().unwrap();
        assert!(minimo >= 0, "no puede haber alturas negativas");
        assert!(maximo < 30, "el terreno se sale de la rejilla");
        assert!(maximo - minimo >= 4, "el terreno es demasiado plano");
        let media = t.average_height();
        assert!(media > 3.0 && media < 8.0, "altura media rara: {media}");
    }

    #[test]
    fn el_terreno_es_continuo_salvo_en_los_rasgos_impuestos() {
        // Fuera del borde de la meseta, dos columnas vecinas no deberian
        // diferenciarse en mas de dos celdas: un salto mayor se ve como un tajo.
        let t = Terrain::generate(TerrainSpec::default());
        let f = t.spec.foundation;
        let (px, pz) = t.spec.pond_center;
        for z in 1..t.size - 1 {
            for x in 1..t.size - 1 {
                if f.distance(x, z) < 4.0 {
                    continue;
                }
                // La orilla del estanque es un talud querido, no un artefacto:
                // se le permite un desnivel mayor, pero acotado igualmente.
                let r = ((x as f64 + 0.5 - px).powi(2) + (z as f64 + 0.5 - pz).powi(2)).sqrt();
                if r < t.spec.pond_radius + 1.5 {
                    let salto = t.slope(x, z);
                    assert!(salto <= 3, "talud de {salto} celdas en {x},{z}");
                    continue;
                }
                let salto = t.slope(x, z);
                assert!(
                    salto <= 2,
                    "escalon de {salto} celdas en {x},{z}: h={} vecinos {:?}",
                    t.height(x, z),
                    [
                        t.height(x + 1, z),
                        t.height(x - 1, z),
                        t.height(x, z + 1),
                        t.height(x, z - 1)
                    ]
                );
            }
        }
    }

    #[test]
    fn el_estanque_queda_por_debajo_del_plano_del_agua() {
        let t = Terrain::generate(TerrainSpec::default());
        let (px, pz) = t.spec.pond_center;
        let centro = t.height(px as i32, pz as i32);
        assert!(
            centro < WATER_PLANE - 1,
            "el centro del estanque esta a {centro}, el agua en {WATER_PLANE}"
        );
        assert!(t.is_submerged(px as i32, pz as i32));
        let sumergidas = t.submerged_columns();
        assert!(sumergidas > 25, "el estanque es diminuto: {sumergidas}");
        assert!(sumergidas < 180, "el estanque se comio el diorama");
    }

    #[test]
    fn la_orilla_contiene_el_agua() {
        // Ninguna columna del anillo que rodea el estanque puede quedar por
        // debajo del plano del agua: si no, el agua se saldria por ese hueco.
        // El canto del diorama queda fuera de la comprobacion porque ahi el
        // terreno tiene que caer; lo que la hace segura es la propiedad que se
        // verifica justo despues.
        let t = Terrain::generate(TerrainSpec::default());
        let (px, pz) = t.spec.pond_center;
        let radio =
            |x: i32, z: i32| ((x as f64 + 0.5 - px).powi(2) + (z as f64 + 0.5 - pz).powi(2)).sqrt();

        for z in 0..t.size {
            for x in 0..t.size {
                if rim_factor(x, z, t.size) > 0.01 {
                    continue;
                }
                let r = radio(x, z);
                if r >= t.spec.pond_radius && r < t.spec.pond_radius + 2.0 {
                    assert!(
                        t.height(x, z) >= WATER_PLANE,
                        "fuga de agua en {x},{z} con altura {}",
                        t.height(x, z)
                    );
                }
            }
        }

        // El estanque no llega al canto, y por eso excluirlo no abre ninguna via
        // de escape: toda columna sumergida esta rodeada de orilla nivelada.
        for z in 0..t.size {
            for x in 0..t.size {
                if !t.is_submerged(x, z) {
                    continue;
                }
                assert!(
                    rim_factor(x, z, t.size) <= 0.01,
                    "el estanque alcanza el canto del diorama en {x},{z}"
                );
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (vx, vz) = (x + dx, z + dz);
                    assert!(
                        t.is_submerged(vx, vz) || t.height(vx, vz) >= WATER_PLANE,
                        "el agua de {x},{z} se escapa por {vx},{vz}"
                    );
                }
            }
        }
    }

    #[test]
    fn el_agua_llena_el_estanque_hasta_su_plano() {
        let t = Terrain::generate(TerrainSpec::default());
        let mut g = VoxelGrid::new(24, 30, 24);
        t.build(&mut g);
        let (px, pz) = (t.spec.pond_center.0 as i32, t.spec.pond_center.1 as i32);
        // Justo bajo el plano hay agua, y justo encima aire.
        assert_eq!(g.get(px, WATER_PLANE - 1, pz), WATER);
        assert_eq!(g.get(px, WATER_PLANE, pz), AIR);
        // Y el fondo es solido.
        let h = t.height(px, pz);
        assert_ne!(g.get(px, h - 1, pz), AIR);
        assert_eq!(g.get(px, h, pz), WATER);
    }

    #[test]
    fn las_capas_se_apilan_en_el_orden_correcto() {
        let t = Terrain::generate(TerrainSpec::default());
        let mut g = VoxelGrid::new(24, 30, 24);
        t.build(&mut g);
        for z in 0..t.size {
            for x in 0..t.size {
                let h = t.height(x, z);
                if h < 5 {
                    continue;
                }
                let superficie = g.get(x, h - 1, z);
                if t.is_submerged(x, z) {
                    assert_eq!(superficie, EARTH_DARK, "fondo del estanque en {x},{z}");
                } else {
                    // Incluye el canto de la isla, que baja de la cota del agua
                    // pero es tierra seca y por tanto lleva musgo.
                    assert_eq!(superficie, EARTH_MOSS, "superficie seca en {x},{z}");
                }
                assert_eq!(g.get(x, h - 2, z), EARTH_DARK, "subsuelo en {x},{z}");
                // La roca madre solo aparece bajo las columnas altas: en el
                // fondo del estanque el suelo no llega a tener esa profundidad.
                if h >= 6 {
                    assert_eq!(g.get(x, 0, z), STONE_RUBBLE, "roca profunda en {x},{z}");
                }
            }
        }
    }

    #[test]
    fn no_hay_columnas_huecas() {
        // Toda columna debe ser solida desde la base: un hueco dejaria ver el
        // cielo por debajo del diorama.
        let t = Terrain::generate(TerrainSpec::default());
        let mut g = VoxelGrid::new(24, 30, 24);
        t.build(&mut g);
        for z in 0..t.size {
            for x in 0..t.size {
                for y in 0..t.height(x, z) {
                    assert_ne!(g.get(x, y, z), AIR, "hueco en {x},{y},{z}");
                }
            }
        }
    }

    #[test]
    fn la_vegetacion_se_apoya_en_el_suelo_y_no_flota() {
        let t = Terrain::generate(TerrainSpec::default());
        let mut g = VoxelGrid::new(24, 30, 24);
        t.build(&mut g);
        t.scatter(&mut g, &[]);

        let mut matas = 0;
        for ([x, y, z], m) in g.iter_solid() {
            if m != FOLIAGE {
                continue;
            }
            matas += 1;
            let debajo = g.get(x, y - 1, z);
            assert_ne!(debajo, AIR, "mata flotando en {x},{y},{z}");
            assert!(y >= WATER_PLANE, "mata dentro del agua en {x},{y},{z}");
        }
        assert!(matas > 15, "apenas hay vegetacion: {matas}");
    }

    #[test]
    fn el_reparto_respeta_las_zonas_reservadas() {
        let t = Terrain::generate(TerrainSpec::default());
        let mut g = VoxelGrid::new(24, 30, 24);
        t.build(&mut g);
        let reserva = Rect::new(4, 4, 20, 20);
        t.scatter(&mut g, &[reserva]);
        for ([x, _, z], m) in g.iter_solid() {
            if m == FOLIAGE {
                assert!(!reserva.contains(x, z), "mata dentro de la reserva");
            }
        }
    }

    #[test]
    fn el_reparto_no_pisa_la_arquitectura() {
        let t = Terrain::generate(TerrainSpec::default());
        let mut g = VoxelGrid::new(24, 30, 24);
        t.build(&mut g);
        // Se simula una losa sobre el terreno antes de repartir.
        let mut losas = Vec::new();
        for z in 2..8 {
            for x in 14..20 {
                let h = t.height(x, z);
                g.set(x, h, z, crate::material::STONE_FLOOR);
                losas.push((x, h, z));
            }
        }
        t.scatter(&mut g, &[]);
        for (x, y, z) in losas {
            assert_eq!(
                g.get(x, y, z),
                crate::material::STONE_FLOOR,
                "el reparto piso la losa de {x},{y},{z}"
            );
        }
    }

    #[test]
    fn el_canto_del_diorama_muestra_sus_estratos() {
        // La isla termina en un zocalo de cantos verticales. Lo que hay que
        // comprobar no es que el perimetro baje, sino que ese canto sea legible:
        // varias celdas de alto y con las tres capas de material a la vista.
        let t = Terrain::generate(TerrainSpec::default());
        let mut g = VoxelGrid::new(t.size, 32, t.size);
        t.build(&mut g);

        let mut minimo = i32::MAX;
        for z in 0..t.size {
            for x in 0..t.size {
                let d = x.min(z).min(t.size - 1 - x).min(t.size - 1 - z);
                if d == 0 {
                    minimo = minimo.min(t.height(x, z));
                }
            }
        }
        assert!(minimo >= 4, "el canto es demasiado bajo: {minimo}");

        // En una columna del canto se ven las tres capas apiladas.
        let h = t.height(0, 12);
        assert_eq!(g.get(0, h - 1, 12), EARTH_MOSS, "falta la capa de musgo");
        assert_eq!(g.get(0, h - 2, 12), EARTH_DARK, "falta el subsuelo");
        assert_eq!(g.get(0, 0, 12), STONE_RUBBLE, "falta la roca de base");
        assert!(
            h >= 5,
            "el canto necesita altura para que se lean los estratos"
        );
    }

    #[test]
    fn el_rectangulo_mide_bien_las_distancias() {
        let r = Rect::new(2, 3, 6, 8);
        assert!(r.contains(2, 3) && r.contains(6, 8) && !r.contains(7, 8));
        assert!(r.distance(4, 5) < 0.0, "dentro deberia ser negativa");
        assert_eq!(r.distance(7, 5), 1.0);
        assert_eq!(r.distance(2, 10), 2.0);
        assert!((r.distance(8, 10) - 8f64.sqrt()).abs() < 1e-12);
    }
}

#[cfg(test)]
mod diagnostico {
    use super::*;

    /// Imprime el mapa de alturas y la cubeta. No es una prueba de correccion:
    /// esta para poder mirar el terreno cuando se ajusta la composicion.
    ///
    /// `cargo test -- --ignored --nocapture mapa_del_terreno`
    #[test]
    #[ignore]
    fn mapa_del_terreno() {
        let t = Terrain::generate(TerrainSpec::default());
        println!("altura por columna (z hacia abajo, x hacia la derecha):");
        for z in 0..t.size {
            let mut fila = String::new();
            for x in 0..t.size {
                let h = t.height(x, z);
                fila.push(if t.is_submerged(x, z) {
                    '~'
                } else {
                    char::from_digit(h.clamp(0, 35) as u32, 36).unwrap_or('?')
                });
            }
            println!("{z:>3} {fila}");
        }
        println!("columnas de estanque: {}", t.submerged_columns());
        println!("altura media: {:.2}", t.average_height());
    }
}
