//! Construccion del diorama: la abadia, la torre, el estanque y los detalles.
//!
//! El terreno lo genera [`crate::terrain`]; aqui se coloca la arquitectura sobre
//! el, bloque a bloque. Los arcos, las escaleras, los contrafuertes y el remate de
//! la torre no son primitivas: se dibujan con cubos colocados a proposito, que es
//! lo que pide el encargo.
//!
//! El orden importa. Primero el terreno, luego la arquitectura, y solo al final el
//! reparto de vegetacion y escombros, que exige celda libre: asi nada brota dentro
//! de un muro ni sobre una losa.

use crate::acceleration::VoxelGrid;
use crate::lighting::Lighting;
use crate::material::{
    MaterialSet, AIR, ALTAR_CRYSTAL, EARTH_MOSS, FOLIAGE, LANTERN, METAL_AGED, STAINED_GLASS,
    STONE_ANCIENT, STONE_FLOOR, STONE_RUBBLE, WOOD_AGED,
};
use crate::math::hash01_3;
use crate::renderer::World;
use crate::skybox::{Skybox, FACE_NAMES};
use crate::terrain::{Rect, Terrain, TerrainSpec, FOUNDATION_HEIGHT, WATER_PLANE};
use crate::texture::{Encoding, Filter, Texture};
use std::path::Path;

/// Altura del suelo de la abadia: la primera celda libre sobre la meseta.
pub const SUELO: i32 = FOUNDATION_HEIGHT;

/// Huella de la nave, incluidos sus muros.
pub const NAVE: Rect = Rect::new(10, 12, 17, 20);
/// Huella de la torre.
pub const TORRE: Rect = Rect::new(18, 16, 22, 21);
/// Plano de la fachada, la cara que mira a la camara.
pub const FACHADA_Z: i32 = 12;
/// Fila del diorama por la que cruza la pasarela de madera.
pub const PASARELA_Z: i32 = 5;
/// Columna de vidrio mas a la izquierda del vitral.
pub const VITRAL_X0: i32 = 11;
/// Columna de vidrio mas a la derecha del vitral.
pub const VITRAL_X1: i32 = 15;
/// Fila inferior del vitral.
pub const VITRAL_Y0: i32 = 13;
/// Fila superior del vitral.
pub const VITRAL_Y1: i32 = 18;

/// Rellena una caja de celdas, extremos incluidos.
pub fn fill_box(g: &mut VoxelGrid, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, m: u16) {
    for z in z0.min(z1)..=z0.max(z1) {
        for y in y0.min(y1)..=y0.max(y1) {
            for x in x0.min(x1)..=x0.max(x1) {
                g.set(x, y, z, m);
            }
        }
    }
}

/// Perfil de un arco de medio punto, en celdas sobre la imposta.
///
/// Para una abertura de semiluz `half`, la altura del arco en la columna a
/// distancia `d` del eje es `round(rise * sqrt(1 - (d/half)^2))`. Redondear sobre
/// una circunferencia es exactamente lo que produce la escalera de bloques que se
/// quiere ver: un arco dibujado con cubos, no una curva suavizada.
#[inline]
pub fn arch_rise(d: f64, half: f64, rise: f64) -> i32 {
    if half <= 0.0 {
        return 0;
    }
    let t = (d / half).clamp(-1.0, 1.0);
    (rise * (1.0 - t * t).max(0.0).sqrt()).round() as i32
}

/// Abre un arco en un muro contenido en un plano `z` constante.
///
/// `x0..=x1` es la luz del vano, `y0` su arranque y `rise` la flecha.
pub fn carve_arch_z(g: &mut VoxelGrid, x0: i32, x1: i32, y0: i32, rise: i32, z: i32, grosor: i32) {
    let centro = (x0 + x1) as f64 * 0.5;
    let half = (x1 - x0) as f64 * 0.5 + 0.5;
    for x in x0..=x1 {
        let alto = arch_rise(x as f64 + 0.5 - centro - 0.5, half, rise as f64);
        for dz in 0..grosor {
            for y in y0..=y0 + alto {
                g.set(x, y, z + dz, AIR);
            }
        }
    }
}

/// Abre un arco en un muro contenido en un plano `x` constante.
pub fn carve_arch_x(g: &mut VoxelGrid, z0: i32, z1: i32, y0: i32, rise: i32, x: i32, grosor: i32) {
    let centro = (z0 + z1) as f64 * 0.5;
    let half = (z1 - z0) as f64 * 0.5 + 0.5;
    for z in z0..=z1 {
        let alto = arch_rise(z as f64 + 0.5 - centro - 0.5, half, rise as f64);
        for dx in 0..grosor {
            for y in y0..=y0 + alto {
                g.set(x + dx, y, z, AIR);
            }
        }
    }
}

/// Columna con basa y capitel, los dos de una celda mas de vuelo.
pub fn columna(g: &mut VoxelGrid, x: i32, z: i32, y0: i32, y1: i32, m: u16) {
    fill_box(g, x, y0, z, x, y1, z, m);
    // Basa y capitel sobresalen en las cuatro direcciones.
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        g.set_if_empty(x + dx, y0, z + dz, m);
        g.set_if_empty(x + dx, y1, z + dz, m);
    }
}

/// Contrafuerte escalonado apoyado contra un muro en un plano `x` constante.
///
/// Cada tramo sube y se retranquea una celda, que es como se descarga el empuje de
/// una boveda y, visualmente, lo que da el perfil de escalera del gotico.
pub fn contrafuerte(g: &mut VoxelGrid, x_muro: i32, z: i32, y0: i32, altura: i32, hacia: i32) {
    let peldanos = (altura / 2).max(2);
    for p in 0..peldanos {
        let vuelo = (peldanos - p).min(3);
        let cima = y0 + 1 + p * 2;
        for d in 1..=vuelo {
            fill_box(
                g,
                x_muro + hacia * d,
                y0,
                z,
                x_muro + hacia * d,
                cima,
                z,
                STONE_ANCIENT,
            );
        }
    }
}

/// Escalera de bloques que sube un desnivel en la direccion `+z`.
pub fn escalera(g: &mut VoxelGrid, x0: i32, x1: i32, z0: i32, y0: i32, peldanos: i32, m: u16) {
    for p in 0..peldanos {
        fill_box(g, x0, y0 + p, z0 + p, x1, y0 + p, z0 + p, m);
    }
}

/// Farol sobre poste de madera con herraje de bronce.
pub fn farol(g: &mut VoxelGrid, x: i32, z: i32, base: i32, altura: i32) {
    fill_box(g, x, base, z, x, base + altura - 1, z, WOOD_AGED);
    g.set(x, base + altura, z, METAL_AGED);
    g.set(x, base + altura + 1, z, LANTERN);
    g.set(x, base + altura + 2, z, METAL_AGED);
}

/// Parametros de composicion del diorama.
#[derive(Debug, Clone)]
pub struct SceneSpec {
    pub terrain: TerrainSpec,
    /// Alto de la rejilla. Tiene que dar cabida al remate de la torre.
    pub height: i32,
}

impl Default for SceneSpec {
    fn default() -> SceneSpec {
        SceneSpec {
            terrain: TerrainSpec::default(),
            height: 30,
        }
    }
}

impl SceneSpec {
    /// Cambia la semilla del terreno.
    pub fn with_seed(mut self, seed: u64) -> SceneSpec {
        self.terrain.seed = seed;
        self
    }
}

/// Resultado de construir la escena.
pub struct Scene {
    pub grid: VoxelGrid,
    pub terrain: Terrain,
}

/// Construye la rejilla completa: terreno, arquitectura y detalles.
pub fn build_scene(spec: &SceneSpec) -> Scene {
    let terrain = Terrain::generate(spec.terrain.clone());
    let n = spec.terrain.size;
    let mut g = VoxelGrid::new(n, spec.height, n);

    terrain.build(&mut g);
    camino(&mut g, &terrain);
    abadia(&mut g);
    torre(&mut g);
    claustro_y_ruinas(&mut g, &terrain);
    estanque(&mut g, &terrain);
    lapidas(&mut g, &terrain);

    // El reparto va al final y evita la huella construida.
    let reservas = [
        NAVE,
        TORRE,
        Rect::new(3, 1, 17, 11),   // estanque, puente y camino
        Rect::new(17, 10, 22, 15), // atrio y placa
    ];
    terrain.scatter(&mut g, &reservas);

    Scene { grid: g, terrain }
}

/// Camino de losa desde el borde del diorama hasta el atrio.
///
/// Se dibuja sustituyendo la celda de superficie de cada columna, no apilando una
/// losa encima: asi el camino queda enrasado con el terreno y no aparece un
/// escalon de una celda a lo largo de todo el trazado.
fn camino(g: &mut VoxelGrid, t: &Terrain) {
    let ruta = [
        (17.0, 3.0),
        (16.5, 4.0),
        (15.5, 7.0),
        (14.5, 9.5),
        (13.5, 11.0),
        (13.0, 12.0),
    ];
    let mut anterior = ruta[0];
    for &punto in &ruta[1..] {
        let pasos = 24;
        for i in 0..=pasos {
            let s = i as f64 / pasos as f64;
            let x = anterior.0 + (punto.0 - anterior.0) * s;
            let z = anterior.1 + (punto.1 - anterior.1) * s;
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let cx = x.round() as i32 + dx;
                    let cz = z.round() as i32 + dz;
                    if (dx.abs() + dz.abs()) > 1 {
                        continue;
                    }
                    let h = t.height(cx, cz);
                    if h < WATER_PLANE || h == 0 {
                        continue;
                    }
                    g.set(cx, h - 1, cz, STONE_FLOOR);
                }
            }
        }
        anterior = punto;
    }
}

/// Nave de la abadia: fachada con vitral, muros, arcadas, altar y cubierta caida.
fn abadia(g: &mut VoxelGrid) {
    let y0 = SUELO;
    let cima = y0 + 11; // coronacion de los muros laterales
    let (x0, x1) = (NAVE.x0, NAVE.x1);
    let (z0, z1) = (NAVE.z0, NAVE.z1);

    // Suelo interior: sustituye la celda de superficie de la meseta.
    fill_box(
        g,
        x0 + 1,
        y0 - 1,
        z0 + 1,
        x1 - 1,
        y0 - 1,
        z1 - 1,
        STONE_FLOOR,
    );

    // Muros laterales y testero.
    fill_box(g, x0, y0, z0, x0, cima, z1, STONE_ANCIENT);
    fill_box(g, x1, y0, z0, x1, cima, z1, STONE_ANCIENT);
    fill_box(g, x0, y0, z1, x1, cima, z1, STONE_ANCIENT);

    // Fachada, mas alta porque remata en hastial.
    let cima_fachada = y0 + 13;
    fill_box(g, x0, y0, z0, x1, cima_fachada, z0, STONE_ANCIENT);
    // Hastial: dos retranqueos que estrechan el remate.
    fill_box(
        g,
        x0 + 1,
        cima_fachada + 1,
        z0,
        x1 - 1,
        cima_fachada + 1,
        z0,
        STONE_ANCIENT,
    );
    fill_box(
        g,
        x0 + 2,
        cima_fachada + 2,
        z0,
        x1 - 2,
        cima_fachada + 2,
        z0,
        STONE_ANCIENT,
    );
    fill_box(
        g,
        x0 + 3,
        cima_fachada + 3,
        z0,
        x1 - 3,
        cima_fachada + 3,
        z0,
        STONE_ANCIENT,
    );

    // Portada: vano con arco de medio punto.
    fill_box(g, 12, y0, z0, 14, y0 + 1, z0, AIR);
    carve_arch_z(g, 12, 14, y0 + 2, 2, z0, 1);
    // Jambas y dovelas de losa, para que la portada se lea como pieza aparte.
    fill_box(g, 11, y0, z0, 11, y0 + 5, z0, STONE_FLOOR);
    fill_box(g, 15, y0, z0, 15, y0 + 5, z0, STONE_FLOOR);

    // Hueco del vitral y su vidriera.
    fill_box(g, VITRAL_X0, VITRAL_Y0, z0, VITRAL_X1, VITRAL_Y1, z0, AIR);
    fill_box(
        g,
        VITRAL_X0,
        VITRAL_Y0,
        z0,
        VITRAL_X1,
        VITRAL_Y1,
        z0,
        STAINED_GLASS,
    );
    // Alfeizar y arco de descarga sobre la vidriera.
    fill_box(
        g,
        VITRAL_X0 - 1,
        VITRAL_Y0 - 1,
        z0,
        VITRAL_X1 + 1,
        VITRAL_Y0 - 1,
        z0,
        STONE_FLOOR,
    );
    for x in VITRAL_X0 - 1..=VITRAL_X1 + 1 {
        let d = x as f64 + 0.5 - (VITRAL_X0 + VITRAL_X1) as f64 * 0.5 - 0.5;
        let alto = arch_rise(d, 3.5, 2.0);
        for y in VITRAL_Y1 + 1..=VITRAL_Y1 + 1 + alto {
            g.set(x, y, z0, STONE_FLOOR);
        }
    }

    // Muro lateral izquierdo derrumbado: es la ventana por la que se ve el
    // interior desde la camara.
    for z in z0 + 1..=z0 + 5 {
        let corte = y0 + 3 + ((z * 7) % 3);
        fill_box(g, x0, corte, z, x0, cima, z, AIR);
        // Coronacion irregular de la ruina.
        if (z * 5) % 3 == 0 {
            g.set(x0, corte, z, STONE_RUBBLE);
        }
    }

    // Ventanales del muro derecho, con arco.
    for z in [z0 + 3, z0 + 6] {
        fill_box(g, x1, y0 + 4, z, x1, y0 + 7, z + 1, AIR);
        carve_arch_x(g, z, z + 1, y0 + 4, 2, x1, 1);
    }

    // Arcadas interiores: pilares y arcos que corren a lo largo de la nave.
    for x in [x0 + 2, x1 - 2] {
        for z in [z0 + 2, z0 + 5] {
            columna(g, x, z, y0, y0 + 5, STONE_ANCIENT);
        }
        // Arco entre los dos pilares de cada lado.
        fill_box(g, x, y0 + 6, z0 + 2, x, y0 + 8, z0 + 5, STONE_ANCIENT);
        carve_arch_x(g, z0 + 3, z0 + 4, y0 + 6, 2, x, 1);
    }

    // Altar al fondo, elevado un peldano y coronado por cristales.
    fill_box(g, 12, y0, z1 - 2, 15, y0, z1 - 1, STONE_FLOOR);
    fill_box(g, 12, y0 + 1, z1 - 1, 15, y0 + 1, z1 - 1, STONE_ANCIENT);
    fill_box(g, 13, y0 + 2, z1 - 1, 14, y0 + 2, z1 - 1, ALTAR_CRYSTAL);
    g.set(13, y0 + 1, z1 - 2, ALTAR_CRYSTAL);

    // Cubierta arruinada: solo quedan tres vigas cruzando la nave.
    for z in [z0 + 2, z0 + 5, z1 - 2] {
        fill_box(g, x0, cima, z, x1, cima, z, WOOD_AGED);
    }
    // Y un tramo de tablazon que aun se sostiene junto al testero.
    fill_box(g, x0 + 1, cima, z1 - 3, x1 - 1, cima, z1 - 1, WOOD_AGED);

    // Contrafuertes del muro derecho.
    for z in [z0 + 2, z0 + 5] {
        contrafuerte(g, x1, z, y0, 8, 1);
    }

    // Escalinata de acceso y atrio de losa delante de la portada.
    escalera(g, 12, 14, z0 - 2, y0 - 1, 1, STONE_FLOOR);
    fill_box(g, 11, y0 - 1, z0 - 2, 15, y0 - 1, z0 - 1, STONE_FLOOR);

    // Faroles flanqueando la portada.
    farol(g, 11, z0 - 1, y0, 2);
    farol(g, 15, z0 - 1, y0, 2);

    // Murete del atrio con la placa de bronce, orientada hacia la camara para que
    // devuelva el estanque, el puente y el cielo del poniente.
    fill_box(g, 17, y0 - 1, z0 - 2, 20, y0 + 3, z0 - 2, STONE_ANCIENT);
    fill_box(g, 17, y0, z0 - 3, 19, y0 + 2, z0 - 3, METAL_AGED);
    fill_box(g, 16, y0 - 1, z0 - 3, 16, y0 + 3, z0 - 2, STONE_ANCIENT);
    fill_box(g, 20, y0 + 4, z0 - 2, 20, y0 + 4, z0 - 2, STONE_RUBBLE);
}

/// Torre parcialmente derrumbada, el punto focal de la composicion.
fn torre(g: &mut VoxelGrid) {
    let y0 = SUELO;
    let (x0, x1) = (TORRE.x0, TORRE.x1);
    let (z0, z1) = (TORRE.z0, TORRE.z1);

    // Altura a la que llega el fuste antes de romperse. La coronacion solo puede
    // recortar por debajo de esta cota: escribir escombro por encima dejaria
    // bloques sueltos en el aire.
    let cima = y0 + 20;

    // Fuste hueco.
    for y in y0..=cima {
        fill_box(g, x0, y, z0, x1, y, z1, STONE_ANCIENT);
        fill_box(g, x0 + 1, y, z0 + 1, x1 - 1, y, z1 - 1, AIR);
    }
    // Forjados de madera que aun quedan.
    fill_box(g, x0 + 1, y0 + 5, z0 + 1, x1 - 1, y0 + 5, z1 - 1, WOOD_AGED);
    fill_box(
        g,
        x0 + 1,
        y0 + 10,
        z0 + 1,
        x1 - 2,
        y0 + 10,
        z1 - 2,
        WOOD_AGED,
    );

    // Vanos: uno por planta en las dos caras que ve la camara.
    for piso in 0..3 {
        let y = y0 + 3 + piso * 5;
        fill_box(g, x0, y, z0 + 2, x0, y + 2, z0 + 3, AIR);
        carve_arch_x(g, z0 + 2, z0 + 3, y, 1, x0, 1);
        fill_box(g, x0 + 2, y, z0, x0 + 3, y + 2, z0, AIR);
        carve_arch_z(g, x0 + 2, x0 + 3, y, 1, z0, 1);
    }

    // Coronacion rota: cada columna del muro termina a una altura distinta, y la
    // esquina que mira a la camara se ha venido abajo del todo.
    for z in z0..=z1 {
        for x in x0..=x1 {
            if x != x0 && x != x1 && z != z0 && z != z1 {
                continue;
            }
            let dado = hash01_3(x as i64, z as i64, 3, 0x70FF);
            let mut tope = y0 + 16 + (dado * 5.0) as i32;
            // Derrumbe concentrado en la esquina que mira a la camara: la torre
            // se abre por ahi y deja ver su interior.
            let dist = (((x - x0).pow(2) + (z - z0).pow(2)) as f64).sqrt();
            if dist < 3.2 {
                tope = y0 + 8 + (dado * 4.0) as i32;
            }
            let tope = tope.min(cima);
            fill_box(g, x, tope + 1, z, x, cima, z, AIR);
            // El remate solo se apoya si tiene fabrica debajo: si el vano de una
            // planta llega justo hasta aqui, la piedra quedaria en el aire.
            if g.get(x, tope - 1, z) != AIR {
                g.set(x, tope, z, STONE_RUBBLE);
            } else {
                g.set(x, tope, z, AIR);
            }
        }
    }

    // Escombro al pie de la torre, del lado del derrumbe.
    for i in 0..14 {
        let dx = (hash01_3(i, 1, 0, 0x5EED) * 5.0) as i32;
        let dz = (hash01_3(i, 2, 0, 0x5EED) * 5.0) as i32;
        let x = x0 - 2 + dx;
        let z = z0 - 2 + dz;
        let h = altura_libre(g, x, z);
        if h > 0 && h < y0 + 3 {
            g.set(x, h, z, STONE_RUBBLE);
            if hash01_3(i, 3, 0, 0x5EED) < 0.3 {
                g.set(x, h + 1, z, STONE_RUBBLE);
            }
        }
    }
}

/// Primera celda libre sobre la columna, o cero si no hay apoyo.
fn altura_libre(g: &VoxelGrid, x: i32, z: i32) -> i32 {
    let [_, ny, _] = g.dims();
    let mut ultima_solida = -1;
    for y in 0..ny {
        if g.get(x, y, z) != AIR {
            ultima_solida = y;
        }
    }
    if ultima_solida < 0 {
        0
    } else {
        ultima_solida + 1
    }
}

/// Fragmentos de muro y restos de claustro a la izquierda de la abadia.
fn claustro_y_ruinas(g: &mut VoxelGrid, t: &Terrain) {
    // Tres lienzos de muro en pie, cada vez mas bajos.
    let lienzos = [(5, 14, 5), (5, 18, 3), (8, 20, 4)];
    for (x, z, alto) in lienzos {
        let h = t.height(x, z);
        if h <= WATER_PLANE {
            continue;
        }
        for dz in 0..3 {
            let hz = t.height(x, z + dz);
            if hz <= WATER_PLANE {
                continue;
            }
            let recorte = ((dz * 7) % 2) as i32;
            fill_box(
                g,
                x,
                hz,
                z + dz,
                x,
                hz + alto - recorte,
                z + dz,
                STONE_ANCIENT,
            );
            g.set(x, hz + alto - recorte, z + dz, STONE_RUBBLE);
        }
    }

    // Restos de una arqueria del claustro: dos pilares y su arco.
    let base = t.height(8, 15).max(WATER_PLANE);
    columna(g, 8, 15, base, base + 3, STONE_ANCIENT);
    columna(g, 8, 18, base, base + 3, STONE_ANCIENT);
    fill_box(g, 8, base + 4, 15, 8, base + 5, 18, STONE_ANCIENT);
    carve_arch_x(g, 16, 17, base + 4, 1, 8, 1);

    // Farol del sendero lateral.
    let hf = t.height(7, 11);
    if hf > WATER_PLANE {
        farol(g, 7, 11, hf, 2);
    }
}

/// Estanque: pasarela de madera sobre el agua, pilotes y piedras sumergidas.
fn estanque(g: &mut VoxelGrid, t: &Terrain) {
    let z = PASARELA_Z;
    let deck = WATER_PLANE + 1;

    // Tablero de la pasarela.
    fill_box(g, 4, deck, z, 15, deck, z, WOOD_AGED);
    // Barandilla discontinua, para que se lea como pasarela y no como un tablon.
    for x in (4..=15).step_by(3) {
        g.set(x, deck + 1, z, WOOD_AGED);
    }
    // Pilotes hasta el fondo.
    for x in [6, 8, 10, 12] {
        let fondo = t.height(x, z);
        fill_box(g, x, fondo, z, x, deck - 1, z, WOOD_AGED);
    }
    // Rampa de union con la orilla este.
    let borde = t.height(15, z);
    if borde < deck {
        fill_box(g, 15, borde, z, 15, deck - 1, z, WOOD_AGED);
    }

    // Piedras bajo el agua, claras y oscuras, para que la refraccion tenga algo
    // reconocible que desplazar. Se colocan sumergidas y sin romper la superficie.
    let piedras = [
        (7, 6, STONE_RUBBLE),
        (9, 7, STONE_ANCIENT),
        (10, 5, STONE_RUBBLE),
        (8, 8, STONE_ANCIENT),
        (11, 7, STONE_RUBBLE),
        (6, 5, STONE_ANCIENT),
        (10, 9, STONE_RUBBLE),
    ];
    for (x, pz, m) in piedras {
        let fondo = t.height(x, pz);
        if fondo >= WATER_PLANE || fondo == 0 {
            continue;
        }
        // Una sola celda, y siempre por debajo del plano del agua.
        if fondo <= WATER_PLANE - 2 {
            g.set(x, fondo, pz, m);
        }
    }

    // Juncos en la orilla.
    for (x, pz) in [(13, 8), (5, 9), (12, 2), (4, 7), (14, 6)] {
        let h = t.height(x, pz);
        if h > WATER_PLANE && g.get(x, h, pz) == AIR {
            g.set(x, h, pz, FOLIAGE);
        }
    }
}

/// Pequeno camposanto, sin nada macabro: losas verticales y musgo.
fn lapidas(g: &mut VoxelGrid, t: &Terrain) {
    let plot = [(3, 14), (4, 16), (3, 18), (5, 20), (6, 17), (4, 12)];
    for (x, z) in plot {
        let h = t.height(x, z);
        if h <= WATER_PLANE || g.get(x, h, z) != AIR {
            continue;
        }
        let alta = hash01_3(x as i64, z as i64, 9, 0x1A9D) < 0.45;
        g.set(x, h, z, STONE_RUBBLE);
        if alta {
            g.set(x, h + 1, z, STONE_RUBBLE);
        }
        // Musgo al pie.
        if g.get(x + 1, h, z) == AIR && g.get(x + 1, h - 1, z) == EARTH_MOSS {
            g.set(x + 1, h, z, FOLIAGE);
        }
    }
}

/// Celdas solidas sin ningun vecino: bloques que se verian flotar.
///
/// No se exige apoyo por debajo, porque un arco, una viga o el tablero de la
/// pasarela se sostienen por sus extremos y no tienen nada bajo el centro. Lo que
/// no puede haber es una celda aislada en el aire.
pub fn floating_blocks(g: &VoxelGrid) -> Vec<[i32; 3]> {
    let mut sueltos = Vec::new();
    for ([x, y, z], _) in g.iter_solid() {
        let vecinos = [
            g.get(x + 1, y, z),
            g.get(x - 1, y, z),
            g.get(x, y + 1, z),
            g.get(x, y - 1, z),
            g.get(x, y, z + 1),
            g.get(x, y, z - 1),
        ];
        if vecinos.iter().all(|&m| m == AIR) {
            sueltos.push([x, y, z]);
        }
    }
    sueltos
}

/// Carga el cubemap del cielo desde los recursos.
pub fn load_skybox(assets: &Path) -> (Skybox, Vec<String>) {
    let dir = assets.join("skybox");
    let mut caras = Vec::with_capacity(6);
    let mut avisos = Vec::new();
    for nombre in FACE_NAMES {
        let ruta = dir.join(format!("sky_{nombre}.ppm"));
        match Texture::load(&ruta, Encoding::Srgb, Filter::Bilinear) {
            Ok(t) => caras.push(t),
            Err(e) => {
                avisos.push(format!("no se pudo cargar {}: {e}", ruta.display()));
                caras.push(Texture::solid(crate::math::v3(0.09, 0.10, 0.20)));
            }
        }
    }
    (Skybox::new(caras, 1.0), avisos)
}

/// Construye el mundo completo listo para renderizar.
pub fn build_world(assets: &Path, spec: &SceneSpec) -> (World, Vec<String>) {
    let (materials, mut avisos) = MaterialSet::load(assets);
    let (skybox, avisos_cielo) = load_skybox(assets);
    avisos.extend(avisos_cielo);

    let escena = build_scene(spec);
    let emisores = Lighting::collect_emitters(&escena.grid, &materials);
    let lighting = Lighting::dusk(emisores);

    (
        World {
            grid: escena.grid,
            materials,
            skybox,
            lighting,
        },
        avisos,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::WATER;
    use std::path::PathBuf;

    fn escena() -> Scene {
        build_scene(&SceneSpec::default())
    }

    #[test]
    fn el_perfil_del_arco_es_una_escalera_de_bloques() {
        // En el eje el arco alcanza su flecha, y en los extremos se cierra.
        assert_eq!(arch_rise(0.0, 2.5, 3.0), 3);
        assert_eq!(arch_rise(2.5, 2.5, 3.0), 0);
        assert_eq!(arch_rise(-2.5, 2.5, 3.0), 0);
        // Y desciende de forma monotona hacia los lados.
        let mut anterior = 99;
        for i in 0..=10 {
            let d = i as f64 * 0.25;
            let a = arch_rise(d, 2.5, 3.0);
            assert!(a <= anterior, "el arco deberia bajar hacia el salmer");
            anterior = a;
        }
        assert_eq!(arch_rise(0.0, 0.0, 3.0), 0, "luz nula no abre arco");
    }

    #[test]
    fn la_escena_se_construye_con_geometria_suficiente() {
        let s = escena();
        let solidas = s.grid.solid_count();
        assert!(solidas > 4000, "el diorama esta vacio: {solidas} celdas");
        assert!(solidas < s.grid.cell_count(), "no puede estar todo relleno");

        // Los doce materiales relevantes tienen que aparecer de verdad.
        let mut vistos = std::collections::HashSet::new();
        for (_, m) in s.grid.iter_solid() {
            vistos.insert(m);
        }
        for (m, nombre) in [
            (STONE_ANCIENT, "piedra antigua"),
            (STONE_FLOOR, "losa"),
            (STONE_RUBBLE, "escombro"),
            (WOOD_AGED, "madera"),
            (EARTH_MOSS, "tierra con musgo"),
            (WATER, "agua"),
            (STAINED_GLASS, "vitral"),
            (METAL_AGED, "bronce"),
            (LANTERN, "farol"),
            (ALTAR_CRYSTAL, "altar"),
            (FOLIAGE, "vegetacion"),
        ] {
            assert!(vistos.contains(&m), "falta {nombre} en la escena");
        }
    }

    #[test]
    fn no_hay_bloques_flotando() {
        let s = escena();
        let sueltos = floating_blocks(&s.grid);
        assert!(
            sueltos.is_empty(),
            "bloques sueltos en el aire: {sueltos:?}"
        );
    }

    #[test]
    fn no_hay_bloques_flotando_con_ninguna_semilla() {
        for seed in [1u64, 42, 20_260_924, 777_777, 9_999_999] {
            let s = build_scene(&SceneSpec::default().with_seed(seed));
            let sueltos = floating_blocks(&s.grid);
            assert!(sueltos.is_empty(), "semilla {seed}: {sueltos:?}");
        }
    }

    #[test]
    fn el_vitral_ocupa_exactamente_su_hueco() {
        let s = escena();
        let mut celdas = 0;
        for ([x, y, z], m) in s.grid.iter_solid() {
            if m != STAINED_GLASS {
                continue;
            }
            celdas += 1;
            assert_eq!(z, FACHADA_Z, "el vitral se salio de la fachada");
            assert!((VITRAL_X0..=VITRAL_X1).contains(&x), "columna {x} fuera");
            assert!((VITRAL_Y0..=VITRAL_Y1).contains(&y), "fila {y} fuera");
        }
        let esperadas = (VITRAL_X1 - VITRAL_X0 + 1) * (VITRAL_Y1 - VITRAL_Y0 + 1);
        assert_eq!(celdas, esperadas, "el vitral deberia llenar su hueco");
    }

    #[test]
    fn el_vitral_coincide_con_el_rectangulo_del_material() {
        // El material estira el dibujo sobre un rectangulo del mundo; la escena
        // tiene que poner el vidrio justo dentro de el, o el rosetón saldria
        // recortado.
        use crate::material::{VITRAL_ALTO, VITRAL_ANCHO, VITRAL_ORIGEN};
        assert_eq!(VITRAL_ORIGEN.x as i32, VITRAL_X0);
        assert_eq!(VITRAL_ORIGEN.x as i32 + VITRAL_ANCHO as i32 - 1, VITRAL_X1);
        assert_eq!(VITRAL_ORIGEN.y as i32 - 1, VITRAL_Y1);
        assert_eq!(VITRAL_ORIGEN.y as i32 - VITRAL_ALTO as i32, VITRAL_Y0);
        assert_eq!(VITRAL_ORIGEN.z as i32, FACHADA_Z);
    }

    #[test]
    fn hay_geometria_detras_del_vitral_para_que_se_note_la_refraccion() {
        let s = escena();
        // Mirando desde la camara hacia el vitral, detras tiene que haber algo:
        // pilares, altar o el muro del testero.
        let mut con_fondo = 0;
        for y in VITRAL_Y0..=VITRAL_Y1 {
            for x in VITRAL_X0..=VITRAL_X1 {
                let hay = (FACHADA_Z + 1..NAVE.z1).any(|z| s.grid.get(x, y, z) != AIR);
                if hay {
                    con_fondo += 1;
                }
            }
        }
        assert!(
            con_fondo >= 6,
            "solo {con_fondo} celdas del vitral tienen fondo detras"
        );
    }

    #[test]
    fn el_altar_esta_dentro_y_alumbra_el_interior() {
        let s = escena();
        let mut celdas = Vec::new();
        for ([x, y, z], m) in s.grid.iter_solid() {
            if m == ALTAR_CRYSTAL {
                celdas.push([x, y, z]);
            }
        }
        assert!(celdas.len() >= 3, "el altar es demasiado pequeno");
        for c in &celdas {
            assert!(
                NAVE.contains(c[0], c[2]),
                "el altar salio de la nave: {c:?}"
            );
            assert!(c[2] > FACHADA_Z, "el altar deberia estar al fondo");
        }
    }

    #[test]
    fn la_torre_es_el_elemento_mas_alto_y_esta_rota() {
        let s = escena();
        let mut cima_torre = 0;
        let mut cima_resto = 0;
        for ([x, y, z], _) in s.grid.iter_solid() {
            if TORRE.contains(x, z) {
                cima_torre = cima_torre.max(y);
            } else {
                cima_resto = cima_resto.max(y);
            }
        }
        assert!(
            cima_torre > cima_resto + 2,
            "la torre no destaca: {cima_torre} frente a {cima_resto}"
        );

        // Remate irregular: las columnas del muro no acaban todas a la misma
        // altura, que es lo que la hace leer como ruina y no como almena.
        let mut topes = std::collections::HashSet::new();
        for x in [TORRE.x0, TORRE.x1] {
            for z in TORRE.z0..=TORRE.z1 {
                let mut t = 0;
                for y in 0..30 {
                    if s.grid.get(x, y, z) != AIR {
                        t = y;
                    }
                }
                topes.insert(t);
            }
        }
        assert!(topes.len() >= 3, "la coronacion es demasiado regular");
    }

    #[test]
    fn la_fachada_deja_ver_el_interior() {
        // El muro izquierdo esta derrumbado en su tramo delantero: por ahi entra
        // la vista a la nave.
        let s = escena();
        let mut aberturas = 0;
        for z in NAVE.z0 + 1..=NAVE.z0 + 5 {
            for y in SUELO + 4..SUELO + 11 {
                if s.grid.get(NAVE.x0, y, z) == AIR {
                    aberturas += 1;
                }
            }
        }
        assert!(aberturas > 10, "no se ve el interior: {aberturas} celdas");
    }

    #[test]
    fn la_portada_esta_abierta_y_tiene_arco() {
        let s = escena();
        // El vano esta libre.
        for y in SUELO..SUELO + 3 {
            for x in 12..=14 {
                assert_eq!(
                    s.grid.get(x, y, FACHADA_Z),
                    AIR,
                    "portada tapiada en {x},{y}"
                );
            }
        }
        // Y por encima hay piedra: el arco cierra el vano.
        assert_ne!(s.grid.get(13, SUELO + 7, FACHADA_Z), AIR);
        // El arco es escalonado: la columna del eje se abre mas que las de los
        // lados.
        let altura_libre = |x: i32| {
            (SUELO..SUELO + 12)
                .take_while(|&y| s.grid.get(x, y, FACHADA_Z) == AIR)
                .count()
        };
        assert!(
            altura_libre(13) > altura_libre(12),
            "el arco no tiene curva"
        );
    }

    #[test]
    fn la_placa_de_bronce_es_grande_y_mira_a_la_camara() {
        let s = escena();
        let mut celdas = Vec::new();
        for ([x, y, z], m) in s.grid.iter_solid() {
            if m == METAL_AGED {
                celdas.push([x, y, z]);
            }
        }
        // Descontando los herrajes de los faroles, la placa tiene que ser un
        // rectangulo de varias celdas.
        let placa: Vec<_> = celdas.iter().filter(|c| c[2] == FACHADA_Z - 3).collect();
        assert!(placa.len() >= 9, "la placa es diminuta: {}", placa.len());

        // Y delante de ella, hacia la camara, no puede haber nada que la tape.
        for c in &placa {
            for z in 0..c[2] {
                assert_eq!(
                    s.grid.get(c[0], c[1], z),
                    AIR,
                    "algo tapa la placa en {},{},{z}",
                    c[0],
                    c[1]
                );
            }
        }
    }

    #[test]
    fn hay_piedras_sumergidas_bajo_la_superficie_del_agua() {
        let s = escena();
        let mut sumergidas = 0;
        for ([x, y, z], m) in s.grid.iter_solid() {
            if m != STONE_RUBBLE && m != STONE_ANCIENT {
                continue;
            }
            if y < WATER_PLANE && s.grid.get(x, y + 1, z) == WATER {
                sumergidas += 1;
            }
        }
        assert!(sumergidas >= 4, "faltan piedras bajo el agua: {sumergidas}");
    }

    #[test]
    fn la_pasarela_cruza_el_agua_y_se_apoya_en_pilotes() {
        let s = escena();
        let deck = WATER_PLANE + 1;
        let mut sobre_agua = 0;
        for x in 4..=15 {
            if s.grid.get(x, deck, PASARELA_Z) == WOOD_AGED
                && s.grid.get(x, WATER_PLANE - 1, PASARELA_Z) == WATER
            {
                sobre_agua += 1;
            }
        }
        assert!(
            sobre_agua >= 4,
            "la pasarela no cruza el agua: {sobre_agua}"
        );

        // Los pilotes llegan al fondo.
        let mut pilotes = 0;
        for x in [6, 8, 10, 12] {
            if s.grid.get(x, WATER_PLANE - 1, PASARELA_Z) == WOOD_AGED {
                pilotes += 1;
            }
        }
        assert!(pilotes >= 3, "faltan pilotes: {pilotes}");
    }

    #[test]
    fn el_camino_llega_de_la_orilla_a_la_portada() {
        let s = escena();
        let t = &s.terrain;
        // Hay losa cerca del borde del diorama y tambien junto a la portada.
        let cerca_borde = (0..9).any(|z| {
            (14..=19).any(|x| {
                let h = t.height(x, z);
                h > 0 && s.grid.get(x, h - 1, z) == STONE_FLOOR
            })
        });
        assert!(cerca_borde, "el camino no arranca en el borde");
        assert_eq!(s.grid.get(13, SUELO - 1, FACHADA_Z - 1), STONE_FLOOR);
    }

    #[test]
    fn hay_faroles_repartidos_y_varios_grupos_emisores() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let (materials, avisos) = MaterialSet::load(&assets);
        assert!(avisos.is_empty());
        let s = escena();
        let emisores = Lighting::collect_emitters(&s.grid, &materials);
        assert!(emisores.len() >= 4, "pocos emisores: {}", emisores.len());
        assert!(
            emisores.len() <= 12,
            "demasiados emisores para muestrear bien"
        );

        // Hay emisores dentro de la nave y tambien fuera.
        let dentro = emisores
            .iter()
            .filter(|e| NAVE.contains(e.center.x as i32, e.center.z as i32))
            .count();
        assert!(dentro >= 1, "el interior esta a oscuras");
        assert!(dentro < emisores.len(), "no hay luces en el exterior");
    }

    #[test]
    fn el_agua_no_se_queda_colgada_sobre_el_aire() {
        let s = escena();
        for ([x, y, z], m) in s.grid.iter_solid() {
            if m != WATER {
                continue;
            }
            let debajo = s.grid.get(x, y - 1, z);
            assert_ne!(debajo, AIR, "agua sin fondo en {x},{y},{z}");
        }
    }

    #[test]
    fn la_arquitectura_se_apoya_en_la_meseta() {
        // Ninguna celda de la nave puede tener aire justo debajo del arranque de
        // los muros: la meseta tiene que sostenerlos.
        let s = escena();
        for z in NAVE.z0..=NAVE.z1 {
            for x in NAVE.x0..=NAVE.x1 {
                assert_ne!(
                    s.grid.get(x, SUELO - 1, z),
                    AIR,
                    "la meseta tiene un hueco en {x},{z}"
                );
            }
        }
    }

    #[test]
    fn la_escena_es_reproducible() {
        let a = build_scene(&SceneSpec::default());
        let b = build_scene(&SceneSpec::default());
        let sa: Vec<_> = a.grid.iter_solid().collect();
        let sb: Vec<_> = b.grid.iter_solid().collect();
        assert_eq!(sa, sb);
    }

    #[test]
    fn se_construye_el_mundo_completo_sin_avisos() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let (mundo, avisos) = build_world(&assets, &SceneSpec::default());
        assert!(avisos.is_empty(), "faltan recursos: {avisos:?}");
        assert_eq!(mundo.skybox.face_resolution(), 256);
        assert!(!mundo.lighting.emitters.is_empty());
        assert!(mundo.grid.solid_count() > 4000);
    }
}
