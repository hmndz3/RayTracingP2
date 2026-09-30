//! Composicion del diorama: ensambla el terreno y la arquitectura.
//!
//! El terreno lo genera [`crate::terrain`] y la canteria vive en
//! [`crate::scene_build`]; aqui se decide el orden y se reune todo en un mundo
//! listo para renderizar.
//!
//! El orden importa. Primero el terreno, luego la arquitectura, y solo al final
//! el reparto de vegetacion y escombros, que exige celda libre: asi nada brota
//! dentro de un muro ni sobre una losa.

use crate::acceleration::VoxelGrid;
use crate::lighting::Lighting;
use crate::material::{MaterialSet, AIR};
use crate::renderer::World;
use crate::skybox::{Skybox, FACE_NAMES};
use crate::terrain::{Rect, Terrain, TerrainSpec};
use crate::texture::{Encoding, Filter, Texture};
use std::path::Path;

pub use crate::scene_build::*;

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
            height: 32,
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
    naves_laterales(&mut g);
    torre(&mut g);
    atrio(&mut g, &terrain);
    ruinas(&mut g, &terrain);
    estanque(&mut g, &terrain);
    lapidas(&mut g, &terrain);

    // El reparto va al final y evita la huella construida.
    let reservas = [
        NAVE,
        AISLE_IZQ,
        AISLE_DER,
        TORRE,
        Rect::new(2, 1, 15, 10),  // estanque y pasarela
        Rect::new(15, 1, 22, 11), // camino y atrio
    ];
    terrain.scatter(&mut g, &reservas);

    Scene { grid: g, terrain }
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
    use crate::material::{
        ALTAR_CRYSTAL, EARTH_MOSS, FOLIAGE, LANTERN, METAL_AGED, STAINED_GLASS, STONE_ANCIENT,
        STONE_FLOOR, STONE_RUBBLE, WOOD_AGED,
    };
    use crate::terrain::WATER_PLANE;
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
            altura_libre(13) > altura_libre(11),
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
        // Se identifica la placa por su plano y por su tramo: el bronce tambien
        // aparece como herraje de los faroles, y esas celdas sueltas no son la
        // placa ni tienen por que estar despejadas.
        let placa: Vec<_> = celdas
            .iter()
            .filter(|c| c[2] == FACHADA_Z - 4 && (17..=20).contains(&c[0]))
            .collect();
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
        for x in 2..=13 {
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

        // Los pilotes apean el tablero hasta el terreno, sin dejar hueco. Se
        // comprueba la columna entera y no un solo nivel: el pilote de la orilla
        // es corto por estar en la parte somera, y eso es correcto.
        let mut apeados = 0;
        for x in [5, 8] {
            let fondo = s.terrain.height(x, PASARELA_Z);
            assert!(
                fondo <= deck,
                "el pilote de {x} nace por encima del tablero"
            );
            for y in fondo..deck {
                assert_eq!(
                    s.grid.get(x, y, PASARELA_Z),
                    WOOD_AGED,
                    "hueco en el pilote de {x} a la altura {y}"
                );
            }
            if fondo < WATER_PLANE {
                apeados += 1;
            }
        }
        assert!(apeados >= 2, "ningun pilote se mete en el agua: {apeados}");
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
        // El tope existe porque cada punto sombreado recorre la lista entera para
        // ordenarla por importancia: con cientos de emisores ese recorrido
        // dominaria el coste del sombreado.
        assert!(
            emisores.len() <= 28,
            "demasiados emisores: {}",
            emisores.len()
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
        assert_eq!(
            mundo.skybox.face_resolution(),
            crate::texgen::SKY_FACE_SIZE,
            "las caras cargadas deben coincidir con las que genera el proyecto"
        );
        assert!(!mundo.lighting.emitters.is_empty());
        assert!(mundo.grid.solid_count() > 4000);
    }
}
