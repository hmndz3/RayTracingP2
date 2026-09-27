//! Cuerpo constructivo del diorama: helpers de cantería y trazado de la abadia.

use crate::acceleration::VoxelGrid;
use crate::material::{
    AIR, ALTAR_CRYSTAL, EARTH_MOSS, FOLIAGE, LANTERN, METAL_AGED, STAINED_GLASS, STONE_ANCIENT,
    STONE_FLOOR, STONE_RUBBLE, WOOD_AGED,
};
use crate::math::hash01_3;
use crate::terrain::{Rect, Terrain, FOUNDATION_HEIGHT, WATER_PLANE};

/// Altura del suelo de la abadia: la primera celda libre sobre la meseta.
pub const SUELO: i32 = FOUNDATION_HEIGHT;

/// Huella de la nave central, incluidos sus muros.
pub const NAVE: Rect = Rect::new(9, 12, 17, 21);
/// Huella de la nave lateral izquierda.
pub const AISLE_IZQ: Rect = Rect::new(6, 13, 9, 20);
/// Huella de la nave lateral derecha.
pub const AISLE_DER: Rect = Rect::new(17, 13, 20, 17);
/// Huella de la torre.
pub const TORRE: Rect = Rect::new(17, 18, 22, 22);
/// Plano de la fachada, la cara que mira a la camara.
pub const FACHADA_Z: i32 = 12;
/// Fila del diorama por la que cruza la pasarela de madera.
pub const PASARELA_Z: i32 = 8;

/// Coronacion de los muros de la nave central.
pub const NAVE_ALTO: i32 = SUELO + 12;
/// Coronacion de los muros de las naves laterales.
pub const AISLE_ALTO: i32 = SUELO + 5;
/// Coronacion del hastial de la fachada, antes de los retranqueos.
pub const FACHADA_ALTO: i32 = SUELO + 12;
/// Cota a la que llega el fuste de la torre antes de romperse.
pub const TORRE_ALTO: i32 = SUELO + 18;

/// Columna de vidrio mas a la izquierda del vitral.
pub const VITRAL_X0: i32 = 11;
/// Columna de vidrio mas a la derecha del vitral.
pub const VITRAL_X1: i32 = 15;
/// Fila inferior del vitral.
pub const VITRAL_Y0: i32 = 14;
/// Fila superior del vitral.
pub const VITRAL_Y1: i32 = 18;

#[allow(clippy::too_many_arguments)]
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
/// Para una abertura de semiluz `half`, la altura del arco a distancia `d` del
/// eje es `round(rise * sqrt(1 - (d/half)^2))`. Redondear sobre una
/// circunferencia es justo lo que produce la escalera de bloques que se quiere
/// ver: un arco dibujado con cubos, no una curva suavizada.
#[inline]
pub fn arch_rise(d: f64, half: f64, rise: f64) -> i32 {
    if half <= 0.0 {
        return 0;
    }
    let t = (d / half).clamp(-1.0, 1.0);
    (rise * (1.0 - t * t).max(0.0).sqrt()).round() as i32
}

/// Abre un arco en un muro contenido en un plano `z` constante.
pub fn carve_arch_z(g: &mut VoxelGrid, x0: i32, x1: i32, y0: i32, rise: i32, z: i32, grosor: i32) {
    let centro = (x0 + x1) as f64 * 0.5;
    let half = (x1 - x0) as f64 * 0.5 + 0.5;
    for x in x0..=x1 {
        let alto = arch_rise(x as f64 - centro, half, rise as f64);
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
        let alto = arch_rise(z as f64 - centro, half, rise as f64);
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
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        g.set_if_empty(x + dx, y0, z + dz, m);
        g.set_if_empty(x + dx, y1, z + dz, m);
    }
}

/// Contrafuerte escalonado apoyado contra un muro en un plano `x` constante.
///
/// Cada tramo sube y se retranquea una celda, que es como se descarga el empuje
/// de una boveda y, visualmente, lo que da el perfil de escalera del gotico.
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

/// Primera celda libre sobre la columna, o cero si no hay apoyo.
pub fn altura_libre(g: &VoxelGrid, x: i32, z: i32) -> i32 {
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

/// Camino de losa desde el borde del diorama hasta el atrio.
///
/// Se dibuja sustituyendo la celda de superficie de cada columna, no apilando una
/// losa encima: asi el camino queda enrasado con el terreno y no aparece un
/// escalon de una celda a lo largo de todo el trazado.
pub fn camino(g: &mut VoxelGrid, t: &Terrain) {
    let ruta = [
        (19.5, 1.0),
        (18.5, 4.0),
        (17.0, 6.5),
        (15.5, 8.5),
        (14.0, 9.5),
        (13.0, 10.5),
    ];
    let mut anterior = ruta[0];
    for &punto in &ruta[1..] {
        let pasos = 24;
        for i in 0..=pasos {
            let s = i as f64 / pasos as f64;
            let x = anterior.0 + (punto.0 - anterior.0) * s;
            let z = anterior.1 + (punto.1 - anterior.1) * s;
            for dx in -1i32..=1 {
                for dz in -1i32..=1 {
                    if dx.abs() + dz.abs() > 1 {
                        continue;
                    }
                    let cx = x.round() as i32 + dx;
                    let cz = z.round() as i32 + dz;
                    let h = t.height(cx, cz);
                    if h < WATER_PLANE || h == 0 || t.is_submerged(cx, cz) {
                        continue;
                    }
                    g.set(cx, h - 1, cz, STONE_FLOOR);
                }
            }
        }
        anterior = punto;
    }
}

/// Nave central: fachada con vitral, muros, arcadas, altar y cubierta caida.
pub fn abadia(g: &mut VoxelGrid) {
    let y0 = SUELO;
    let (x0, x1) = (NAVE.x0, NAVE.x1);
    let (z0, z1) = (NAVE.z0, NAVE.z1);

    // Suelo interior: sustituye la celda de superficie de la meseta.
    fill_box(g, x0, y0 - 1, z0, x1, y0 - 1, z1, STONE_FLOOR);

    // Muros laterales y testero.
    fill_box(g, x0, y0, z0, x0, NAVE_ALTO, z1, STONE_ANCIENT);
    fill_box(g, x1, y0, z0, x1, NAVE_ALTO, z1, STONE_ANCIENT);
    fill_box(g, x0, y0, z1, x1, NAVE_ALTO, z1, STONE_ANCIENT);

    // Fachada y hastial escalonado.
    fill_box(g, x0, y0, z0, x1, FACHADA_ALTO, z0, STONE_ANCIENT);
    for (paso, retranqueo) in [(1, 1), (2, 2), (3, 3)] {
        fill_box(
            g,
            x0 + retranqueo,
            FACHADA_ALTO + paso,
            z0,
            x1 - retranqueo,
            FACHADA_ALTO + paso,
            z0,
            STONE_ANCIENT,
        );
    }

    // Portada: vano amplio rematado por arco de medio punto.
    fill_box(g, 11, y0, z0, 15, y0 + 2, z0, AIR);
    carve_arch_z(g, 11, 15, y0 + 3, 2, z0, 1);
    // Jambas y dovelas de losa, para que la portada se lea como pieza aparte.
    fill_box(g, 10, y0, z0, 10, y0 + 6, z0, STONE_FLOOR);
    fill_box(g, 16, y0, z0, 16, y0 + 6, z0, STONE_FLOOR);

    // Banda de piedra que separa la portada del vitral.
    fill_box(g, x0, VITRAL_Y0 - 1, z0, x1, VITRAL_Y0 - 1, z0, STONE_FLOOR);

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
    // Arco de descarga sobre la vidriera.
    for x in VITRAL_X0 - 1..=VITRAL_X1 + 1 {
        let alto = arch_rise(x as f64 - (VITRAL_X0 + VITRAL_X1) as f64 * 0.5, 3.5, 2.0);
        for y in VITRAL_Y1 + 1..=VITRAL_Y1 + 1 + alto {
            g.set(x, y, z0, STONE_FLOOR);
        }
    }

    // Arqueria que abre la nave central a las laterales.
    for (x, rango) in [(x0, AISLE_IZQ), (x1, AISLE_DER)] {
        let mut z = rango.z0 + 1;
        while z < rango.z1 - 1 {
            fill_box(g, x, y0, z, x, y0 + 4, z + 1, AIR);
            carve_arch_x(g, z, z + 1, y0 + 4, 2, x, 1);
            z += 3;
        }
    }

    // Claristorio: ventanas altas del muro derecho, por encima del tejado lateral.
    for z in [z0 + 3, z0 + 6, z0 + 9] {
        fill_box(g, x1, y0 + 8, z, x1, y0 + 10, z, AIR);
        carve_arch_x(g, z, z, y0 + 8, 1, x1, 1);
    }

    // Muro izquierdo derrumbado en su tramo delantero: es la abertura por la que
    // la camara ve el interior.
    for z in z0 + 1..=z0 + 4 {
        let corte = y0 + 4 + ((z * 7) % 3);
        fill_box(g, x0, corte + 1, z, x0, NAVE_ALTO, z, AIR);
        g.set(x0, corte, z, STONE_RUBBLE);
    }

    // Pilares exentos de la nave, con su arco longitudinal.
    for x in [x0 + 2, x1 - 2] {
        for z in [z0 + 3, z0 + 7] {
            columna(g, x, z, y0, y0 + 6, STONE_ANCIENT);
        }
        fill_box(g, x, y0 + 7, z0 + 3, x, y0 + 9, z0 + 7, STONE_ANCIENT);
        carve_arch_x(g, z0 + 4, z0 + 6, y0 + 7, 2, x, 1);
    }

    // Presbiterio y altar, elevados un peldano.
    fill_box(g, x0 + 2, y0, z1 - 3, x1 - 2, y0, z1 - 1, STONE_FLOOR);
    fill_box(g, 12, y0 + 1, z1 - 1, 14, y0 + 1, z1 - 1, STONE_ANCIENT);
    fill_box(g, 12, y0 + 2, z1 - 1, 14, y0 + 2, z1 - 1, ALTAR_CRYSTAL);
    g.set(13, y0 + 1, z1 - 2, ALTAR_CRYSTAL);
    // Dos cirios flanqueando el presbiterio.
    for x in [x0 + 2, x1 - 2] {
        fill_box(g, x, y0, z1 - 2, x, y0 + 2, z1 - 2, WOOD_AGED);
        g.set(x, y0 + 3, z1 - 2, LANTERN);
    }

    // Hachones colgados de los muros de la nave.
    //
    // Estan ahi por una razon concreta, no de relleno: el vitral solo se lee
    // desde fuera si hay luz detras de el, y el altar esta al otro extremo de la
    // nave. Los dos primeros quedan justo tras la vidriera y son los que la
    // encienden; los de mas al fondo alargan la perspectiva del interior.
    for (x, hacia) in [(x0 + 1, 1), (x1 - 1, -1)] {
        for (i, z) in [z0 + 1, z0 + 6].into_iter().enumerate() {
            let y = if i == 0 { VITRAL_Y0 + 1 } else { y0 + 6 };
            g.set(x, y, z, METAL_AGED);
            g.set(x + hacia, y, z, LANTERN);
        }
    }

    // Cubierta arruinada: solo quedan algunas vigas cruzando la nave.
    for z in [z0 + 2, z0 + 5, z0 + 8, z1 - 2] {
        fill_box(g, x0, NAVE_ALTO, z, x1, NAVE_ALTO, z, WOOD_AGED);
    }
    fill_box(
        g,
        x0 + 1,
        NAVE_ALTO,
        z1 - 3,
        x1 - 1,
        NAVE_ALTO,
        z1 - 1,
        WOOD_AGED,
    );

    // Contrafuertes del muro derecho, por encima de la nave lateral.
    for z in [z0 + 7, z0 + 9] {
        contrafuerte(g, x1, z, y0, 8, 1);
    }

    // Escalinata y atrio de losa delante de la portada.
    escalera(g, 11, 15, z0 - 2, y0 - 1, 1, STONE_FLOOR);
    fill_box(g, 10, y0 - 1, z0 - 3, 16, y0 - 1, z0 - 1, STONE_FLOOR);

    // Faroles flanqueando la portada.
    farol(g, 10, z0 - 2, y0, 2);
    farol(g, 16, z0 - 2, y0, 2);
}

/// Naves laterales, mas bajas, que ensanchan la base del conjunto.
pub fn naves_laterales(g: &mut VoxelGrid) {
    let y0 = SUELO;
    for (rango, exterior) in [(AISLE_IZQ, AISLE_IZQ.x0), (AISLE_DER, AISLE_DER.x1)] {
        let (z0, z1) = (rango.z0, rango.z1);
        fill_box(g, rango.x0, y0 - 1, z0, rango.x1, y0 - 1, z1, STONE_FLOOR);
        // Muro exterior y testeros.
        fill_box(g, exterior, y0, z0, exterior, AISLE_ALTO, z1, STONE_ANCIENT);
        fill_box(g, rango.x0, y0, z0, rango.x1, AISLE_ALTO, z0, STONE_ANCIENT);
        fill_box(g, rango.x0, y0, z1, rango.x1, AISLE_ALTO, z1, STONE_ANCIENT);
        // Tejado de tablazon en pendiente de una sola agua.
        for (i, x) in (rango.x0..=rango.x1).enumerate() {
            let y = AISLE_ALTO + 1 + (i as i32) / 2;
            fill_box(g, x, y, z0, x, y, z1, WOOD_AGED);
        }
        // Ventanas bajas.
        let mut z = z0 + 2;
        while z <= z1 - 2 {
            fill_box(g, exterior, y0 + 2, z, exterior, y0 + 3, z, AIR);
            z += 3;
        }
    }

    // La nave izquierda esta descubierta en su tramo delantero: desde ahi se ve
    // el interior y la arqueria que la separa de la central.
    for z in AISLE_IZQ.z0..=AISLE_IZQ.z0 + 3 {
        for x in AISLE_IZQ.x0..=AISLE_IZQ.x1 {
            for y in AISLE_ALTO - 1..=AISLE_ALTO + 4 {
                if g.get(x, y, z) == WOOD_AGED || y > AISLE_ALTO - 1 {
                    g.set(x, y, z, AIR);
                }
            }
        }
        g.set(AISLE_IZQ.x0, AISLE_ALTO - 1, z, STONE_RUBBLE);
    }
}

/// Torre parcialmente derrumbada, el punto focal de la composicion.
pub fn torre(g: &mut VoxelGrid) {
    let y0 = SUELO;
    let (x0, x1) = (TORRE.x0, TORRE.x1);
    let (z0, z1) = (TORRE.z0, TORRE.z1);
    let cima = TORRE_ALTO;

    // Fuste hueco.
    for y in y0..=cima {
        fill_box(g, x0, y, z0, x1, y, z1, STONE_ANCIENT);
        fill_box(g, x0 + 1, y, z0 + 1, x1 - 1, y, z1 - 1, AIR);
    }
    fill_box(g, x0, y0 - 1, z0, x1, y0 - 1, z1, STONE_FLOOR);

    // Forjados de madera que aun quedan.
    fill_box(g, x0 + 1, y0 + 6, z0 + 1, x1 - 1, y0 + 6, z1 - 1, WOOD_AGED);
    fill_box(
        g,
        x0 + 1,
        y0 + 12,
        z0 + 1,
        x1 - 2,
        y0 + 12,
        z1 - 2,
        WOOD_AGED,
    );

    // Puerta de paso desde la nave y vanos de cada planta.
    fill_box(g, x0, y0, z0 + 1, x0, y0 + 2, z0 + 2, AIR);
    for piso in 0..3 {
        let y = y0 + 3 + piso * 5;
        fill_box(g, x0, y, z0 + 3, x0, y + 2, z0 + 3, AIR);
        carve_arch_x(g, z0 + 3, z0 + 3, y, 1, x0, 1);
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
            let mut tope = y0 + 14 + (dado * 5.0) as i32;
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

    // Farol en el vano bajo de la torre, que marca el fondo de la composicion.
    // Apoyado en el muro: un farol exento quedaria flotando en el hueco.
    g.set(x0 + 1, y0 + 4, z0 + 1, LANTERN);

    // Escombro al pie de la torre, del lado del derrumbe.
    for i in 0..16 {
        let dx = (hash01_3(i, 1, 0, 0x5EED) * 6.0) as i32;
        let dz = (hash01_3(i, 2, 0, 0x5EED) * 6.0) as i32;
        let x = x0 - 3 + dx;
        let z = z0 - 3 + dz;
        if NAVE.contains(x, z) || TORRE.contains(x, z) {
            continue;
        }
        let h = altura_libre(g, x, z);
        if h > 0 && h <= y0 + 1 {
            g.set(x, h, z, STONE_RUBBLE);
            if hash01_3(i, 3, 0, 0x5EED) < 0.3 {
                g.set(x, h + 1, z, STONE_RUBBLE);
            }
        }
    }
}

/// Atrio amurallado con la placa de bronce, a la derecha de la portada.
pub fn atrio(g: &mut VoxelGrid, t: &Terrain) {
    let y0 = SUELO;
    // Murete que cierra el atrio por el frente.
    fill_box(
        g,
        17,
        y0 - 1,
        FACHADA_Z - 3,
        21,
        y0 + 3,
        FACHADA_Z - 3,
        STONE_ANCIENT,
    );
    fill_box(
        g,
        21,
        y0 - 1,
        FACHADA_Z - 6,
        21,
        y0 + 3,
        FACHADA_Z - 3,
        STONE_ANCIENT,
    );
    g.set(19, y0 + 4, FACHADA_Z - 3, STONE_RUBBLE);

    // Placa de bronce, orientada hacia la camara para que devuelva el estanque,
    // la pasarela y el cielo del poniente.
    fill_box(
        g,
        17,
        y0,
        FACHADA_Z - 4,
        20,
        y0 + 2,
        FACHADA_Z - 4,
        METAL_AGED,
    );

    // Losa del atrio.
    fill_box(
        g,
        17,
        y0 - 1,
        FACHADA_Z - 2,
        21,
        y0 - 1,
        FACHADA_Z - 1,
        STONE_FLOOR,
    );

    // Farol del atrio.
    farol(g, 21, FACHADA_Z - 5, y0, 2);

    // Banco corrido de piedra.
    fill_box(g, 18, y0, FACHADA_Z - 2, 20, y0, FACHADA_Z - 2, STONE_FLOOR);

    // Farol del sendero, ya sobre el terreno natural.
    let h = t.height(19, 4);
    if h > WATER_PLANE {
        farol(g, 19, 4, h, 2);
    }
}

/// Fragmentos de muro y restos de claustro a la izquierda de la abadia.
pub fn ruinas(g: &mut VoxelGrid, t: &Terrain) {
    // Tres lienzos de muro en pie, cada vez mas bajos.
    for (x, z, alto) in [(3, 13, 5), (3, 17, 4), (4, 20, 3)] {
        for dz in 0..3 {
            let hz = t.height(x, z + dz);
            if hz < WATER_PLANE || t.is_submerged(x, z + dz) {
                continue;
            }
            let recorte = (dz * 7) % 2;
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

    // Restos de la arqueria del claustro: dos pilares y su arco.
    let base = t.height(5, 15).max(WATER_PLANE);
    columna(g, 5, 15, base, base + 3, STONE_ANCIENT);
    columna(g, 5, 18, base, base + 3, STONE_ANCIENT);
    fill_box(g, 5, base + 4, 15, 5, base + 5, 18, STONE_ANCIENT);
    carve_arch_x(g, 16, 17, base + 4, 1, 5, 1);

    // Losa del suelo del claustro, ya casi cubierta.
    for z in 15..=18 {
        for x in 4..=7 {
            let h = t.height(x, z);
            if h >= WATER_PLANE && !t.is_submerged(x, z) && g.get(x, h, z) == AIR {
                g.set(x, h - 1, z, STONE_FLOOR);
            }
        }
    }

    // Farol del sendero lateral.
    let hf = t.height(4, 12);
    if hf >= WATER_PLANE && !t.is_submerged(4, 12) {
        farol(g, 4, 12, hf, 2);
    }
}

/// Estanque: pasarela de madera sobre el agua, pilotes y piedras sumergidas.
pub fn estanque(g: &mut VoxelGrid, t: &Terrain) {
    let z = PASARELA_Z;
    let deck = WATER_PLANE + 1;

    fill_box(g, 2, deck, z, 13, deck, z, WOOD_AGED);
    for x in (2..=13).step_by(3) {
        g.set(x, deck + 1, z, WOOD_AGED);
    }
    for x in [5, 8] {
        let fondo = t.height(x, z);
        fill_box(g, x, fondo, z, x, deck - 1, z, WOOD_AGED);
    }
    // Rampas de union con las dos orillas.
    for x in [2, 13] {
        let borde = t.height(x, z);
        if borde < deck {
            fill_box(g, x, borde, z, x, deck - 1, z, WOOD_AGED);
        }
    }

    // Piedras bajo el agua, claras y oscuras, para que la refraccion tenga algo
    // reconocible que desplazar. Siempre por debajo del plano del agua.
    let piedras = [
        (5, 5, STONE_RUBBLE),
        (6, 4, STONE_ANCIENT),
        (8, 6, STONE_RUBBLE),
        (7, 5, STONE_RUBBLE),
        (8, 7, STONE_ANCIENT),
        (9, 5, STONE_RUBBLE),
        (7, 7, STONE_ANCIENT),
        (10, 6, STONE_RUBBLE),
        (6, 6, STONE_ANCIENT),
        (9, 8, STONE_RUBBLE),
        (10, 4, STONE_ANCIENT),
        (8, 4, STONE_RUBBLE),
    ];
    for (x, pz, m) in piedras {
        let fondo = t.height(x, pz);
        if !t.is_submerged(x, pz) || fondo > WATER_PLANE - 2 {
            continue;
        }
        // Solo se sustituye agua: si aqui baja un pilote, colocar la piedra le
        // abriria un hueco y el tablero quedaria apeado en el aire.
        if g.get(x, fondo, pz) != crate::material::WATER {
            continue;
        }
        g.set(x, fondo, pz, m);
    }

    // Juncos en la orilla.
    for (x, pz) in [(12, 6), (2, 6), (12, 3), (2, 3), (13, 8), (4, 2)] {
        let h = t.height(x, pz);
        if h >= WATER_PLANE && !t.is_submerged(x, pz) && g.get(x, h, pz) == AIR {
            g.set(x, h, pz, FOLIAGE);
        }
    }

    // Faroles en la orilla. Estan por el reflejo: son la fuente luminosa que el
    // encargo pide colocar frente al estanque, y lo que hace que la lamina de
    // agua devuelva algo reconocible en lugar de solo cielo.
    for (x, pz) in [(12, 8), (2, 5)] {
        let h = t.height(x, pz);
        if h >= WATER_PLANE && !t.is_submerged(x, pz) && g.get(x, h, pz) == AIR {
            farol(g, x, pz, h, 2);
        }
    }
}

/// Pequeno camposanto, sin nada macabro: losas verticales y musgo.
pub fn lapidas(g: &mut VoxelGrid, t: &Terrain) {
    let plot = [
        (2, 8),
        (3, 10),
        (2, 12),
        (4, 9),
        (3, 7),
        (5, 11),
        (2, 15),
        (4, 22),
        (6, 22),
    ];
    for (x, z) in plot {
        let h = t.height(x, z);
        if h < WATER_PLANE || t.is_submerged(x, z) || g.get(x, h, z) != AIR {
            continue;
        }
        let alta = hash01_3(x as i64, z as i64, 9, 0x1A9D) < 0.45;
        g.set(x, h, z, STONE_RUBBLE);
        if alta {
            g.set(x, h + 1, z, STONE_RUBBLE);
        }
        if g.get(x + 1, h, z) == AIR && g.get(x + 1, h - 1, z) == EARTH_MOSS {
            g.set(x + 1, h, z, FOLIAGE);
        }
    }
}
