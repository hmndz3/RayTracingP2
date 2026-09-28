//! Pruebas de integracion: ejercitan el proyecto desde fuera de la biblioteca,
//! con los recursos reales del repositorio y la escena completa.
//!
//! Las pruebas unitarias de cada modulo trabajan con escenas de laboratorio. Estas
//! comprueban lo que solo se puede comprobar con todo montado: que la imagen
//! final no esta subexpuesta, que los efectos que pide el encargo se ven de
//! verdad en los pixeles, y que cambiar la semilla cambia el diorama.

use abadia::acceleration::VoxelGrid;
use abadia::camera::{tour_camera, tour_keys, Camera};
use abadia::image::Image;
use abadia::material::{AIR, STAINED_GLASS, WATER};
use abadia::math::{Rng, Vec3};
use abadia::ray::{Interval, Ray};
use abadia::renderer::{render, RenderSettings, World};
use abadia::scene::{build_world, floating_blocks, SceneSpec};
use std::path::PathBuf;

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

fn mundo(seed: u64) -> World {
    let (w, avisos) = build_world(&assets(), &SceneSpec::default().with_seed(seed));
    assert!(
        avisos.is_empty(),
        "faltan recursos del repositorio: {avisos:?}"
    );
    w
}

fn ajustes(width: usize, height: usize, samples: usize) -> RenderSettings {
    RenderSettings {
        width,
        height,
        samples,
        max_depth: 6,
        threads: 4,
        tile: 32,
        ..RenderSettings::default()
    }
}

/// Estadisticos de una imagen en el espacio de salida.
struct Estadisticas {
    media: f64,
    minimo: u8,
    maximo: u8,
    /// Fraccion de pixeles casi negros.
    sombras: f64,
    /// Fraccion de pixeles quemados.
    altas: f64,
}

fn estadisticas(img: &Image) -> Estadisticas {
    let mut suma = 0.0;
    let mut minimo = 255u8;
    let mut maximo = 0u8;
    let mut sombras = 0usize;
    let mut altas = 0usize;
    let n = img.width * img.height;
    for p in img.data.chunks(3) {
        let l = (0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64) as u8;
        suma += l as f64;
        minimo = minimo.min(l);
        maximo = maximo.max(l);
        if l < 12 {
            sombras += 1;
        }
        if l > 250 {
            altas += 1;
        }
    }
    Estadisticas {
        media: suma / n as f64,
        minimo,
        maximo,
        sombras: sombras as f64 / n as f64,
        altas: altas as f64 / n as f64,
    }
}

#[test]
fn la_vista_inicial_esta_bien_expuesta() {
    let w = mundo(SceneSpec::default().terrain.seed);
    let mut c = Camera::initial();
    c.enforce_outside(w.grid.bounds(), 1.5);
    let r = render(&w, &c, &ajustes(480, 270, 4), None, None).expect("no deberia cancelarse");
    let img = r.framebuffer.to_image(1.0);
    let e = estadisticas(&img);

    // El encargo insiste en que la escena no quede subexpuesta: es un anochecer,
    // no una imagen oscura.
    assert!(
        e.media > 45.0,
        "la imagen esta subexpuesta: media {}",
        e.media
    );
    assert!(e.media < 165.0, "la imagen esta lavada: media {}", e.media);
    assert!(e.maximo > 200, "faltan altas luces: maximo {}", e.maximo);
    assert!(
        e.sombras < 0.12,
        "demasiado pixel negro: {:.1} %",
        e.sombras * 100.0
    );
    assert!(
        e.altas < 0.06,
        "demasiado pixel quemado: {:.1} %",
        e.altas * 100.0
    );
    assert!(e.minimo < 120, "no hay sombras, la imagen es plana");
}

#[test]
fn la_vista_inicial_encuadra_la_abadia_el_agua_y_las_luces() {
    // Se comprueba sobre la propia imagen, no sobre la escena: lo que importa es
    // que los tres elementos esten dentro del encuadre inicial.
    let w = mundo(SceneSpec::default().terrain.seed);
    let mut c = Camera::initial();
    c.enforce_outside(w.grid.bounds(), 1.5);
    let basis = c.basis(320, 180);

    let mut vistos = [false; 3]; // abadia, agua, emisor
    for y in 0..180 {
        for x in 0..320 {
            let r = basis.ray(x as f64 + 0.5, y as f64 + 0.5);
            if let Some(h) = w.grid.hit(&r, Interval::positive(), AIR) {
                let m = w.materials.get(h.material);
                if h.material == WATER {
                    vistos[1] = true;
                } else if m.is_emissive() {
                    vistos[2] = true;
                } else if h.material == abadia::material::STONE_ANCIENT && h.point.y > 12.0 {
                    vistos[0] = true;
                }
            }
        }
    }
    assert!(vistos[0], "la abadia no entra en la vista inicial");
    assert!(vistos[1], "el estanque no entra en la vista inicial");
    assert!(
        vistos[2],
        "no hay ninguna luz encendida en la vista inicial"
    );
}

#[test]
fn el_agua_refracta_y_deja_ver_el_fondo() {
    // Se lanza un rayo oblicuo contra la superficie del estanque y se sigue el
    // camino refractado: tiene que alcanzar el fondo, y el punto al que llega
    // tiene que estar desplazado respecto del que veria en linea recta.
    let w = mundo(SceneSpec::default().terrain.seed);
    let mut encontrados = 0;

    for i in 0..400 {
        let a = i as f64 * 0.0157;
        let origen = Vec3 {
            x: 7.5 + 9.0 * a.cos(),
            y: 11.0,
            z: 6.0 + 9.0 * a.sin(),
        };
        let objetivo = Vec3 {
            x: 7.5,
            y: 6.0,
            z: 6.0,
        };
        let rayo = Ray::new(origen, objetivo - origen);
        let Some(sup) = w.grid.hit(&rayo, Interval::positive(), AIR) else {
            continue;
        };
        if sup.material != WATER || !sup.front_face {
            continue;
        }

        let m = w.materials.get(WATER);
        let n = sup.facing_normal();
        let Some(dir) = abadia::math::refract(rayo.dir, n, 1.0 / m.ior) else {
            continue;
        };
        let dentro = Ray::new(sup.point + dir * 1e-4, dir);
        let Some(fondo) = w.grid.hit(&dentro, Interval::positive(), WATER) else {
            continue;
        };
        assert_ne!(
            fondo.material, WATER,
            "el agua no puede chocar consigo misma"
        );

        // Camino recto desde el mismo punto, ignorando la refraccion.
        let recto = Ray::new(sup.point + rayo.dir * 1e-4, rayo.dir);
        if let Some(sin_refraccion) = w.grid.hit(&recto, Interval::positive(), WATER) {
            let desplazamiento = (fondo.point - sin_refraccion.point).length();
            if desplazamiento > 0.05 {
                encontrados += 1;
            }
        }
    }
    assert!(
        encontrados > 40,
        "la refraccion apenas desplaza el fondo: {encontrados} casos"
    );
}

#[test]
fn el_vitral_tine_la_luz_que_lo_atraviesa() {
    // Un rayo que cruza la vidriera tiene que salir con el color del panel, no
    // con el color de lo que hay detras.
    let w = mundo(SceneSpec::default().terrain.seed);
    let vidrio = w.materials.get(STAINED_GLASS);
    assert!(vidrio.absorption_from_texture > 0.0);

    let mut tonos = Vec::new();
    for x in 11..=15 {
        for y in 14..=18 {
            let origen = Vec3 {
                x: x as f64 + 0.5,
                y: y as f64 + 0.5,
                z: 6.0,
            };
            let rayo = Ray::new(
                origen,
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
            );
            let Some(h) = w.grid.hit(&rayo, Interval::positive(), AIR) else {
                continue;
            };
            if h.material != STAINED_GLASS {
                continue;
            }
            let tinte = w.materials.albedo_at(vidrio, &h);
            let medio = vidrio.medium(tinte);
            let t = medio.transmittance(0.5);
            assert!(t.is_finite());
            tonos.push(t);
        }
    }
    assert!(
        tonos.len() >= 15,
        "el vitral no se muestreo: {}",
        tonos.len()
    );

    // Hay paneles que dejan pasar sobre todo el rojo y otros sobre todo el azul:
    // el vidrio tine, no solo atenua.
    let calidos = tonos.iter().filter(|t| t.x > t.z * 1.4).count();
    let frios = tonos.iter().filter(|t| t.z > t.x * 1.4).count();
    assert!(calidos >= 3, "faltan paneles calidos: {calidos}");
    assert!(frios >= 3, "faltan paneles frios: {frios}");
}

#[test]
fn los_mapas_normales_cambian_el_sombreado_de_la_piedra() {
    let w = mundo(SceneSpec::default().terrain.seed);
    let mut c = Camera::initial();
    c.yaw = -196.0;
    c.pitch = 9.0;
    c.distance = 17.0;
    c.target = Vec3 {
        x: 9.0,
        y: 10.5,
        z: 13.0,
    };
    c.enforce_outside(w.grid.bounds(), 1.5);

    let con = ajustes(320, 180, 4);
    let mut sin = con.clone();
    sin.normal_maps = false;

    let a = render(&w, &c, &con, None, None)
        .unwrap()
        .framebuffer
        .to_image(1.0);
    let b = render(&w, &c, &sin, None, None)
        .unwrap()
        .framebuffer
        .to_image(1.0);
    assert_ne!(a, b);

    let distintos = a
        .data
        .iter()
        .zip(&b.data)
        .filter(|(x, y)| x.abs_diff(**y) > 3)
        .count();
    let fraccion = distintos as f64 / a.data.len() as f64;
    assert!(
        fraccion > 0.05,
        "el relieve apenas se nota en la imagen: {:.1} %",
        fraccion * 100.0
    );

    // Y el relieve no puede oscurecer la escena en conjunto: solo redistribuye.
    let (ea, eb) = (estadisticas(&a), estadisticas(&b));
    assert!(
        (ea.media - eb.media).abs() < 22.0,
        "el mapa normal cambia la exposicion global: {} vs {}",
        ea.media,
        eb.media
    );
}

#[test]
fn el_render_no_depende_del_numero_de_hilos() {
    let w = mundo(SceneSpec::default().terrain.seed);
    let mut c = Camera::initial();
    c.enforce_outside(w.grid.bounds(), 1.5);

    let mut uno = ajustes(240, 135, 2);
    uno.threads = 1;
    let mut varios = uno.clone();
    varios.threads = 8;

    let a = render(&w, &c, &uno, None, None)
        .unwrap()
        .framebuffer
        .to_image(1.0);
    let b = render(&w, &c, &varios, None, None)
        .unwrap()
        .framebuffer
        .to_image(1.0);
    assert_eq!(a, b, "el reparto entre hilos altera la imagen");
}

#[test]
fn el_diorama_es_reproducible_y_la_semilla_lo_cambia() {
    let a = mundo(20_260_924);
    let b = mundo(20_260_924);
    let c = mundo(7_777);

    let solidas = |w: &World| w.grid.iter_solid().collect::<Vec<_>>();
    assert_eq!(
        solidas(&a),
        solidas(&b),
        "la misma semilla debe dar lo mismo"
    );
    assert_ne!(solidas(&a), solidas(&c), "la semilla deberia cambiar algo");

    // Pero la arquitectura no se mueve: es composicion, no azar.
    let muro = |w: &World| w.grid.get(9, 12, 12);
    assert_eq!(muro(&a), muro(&c));
    assert_ne!(muro(&a), AIR);
}

#[test]
fn ninguna_semilla_deja_bloques_flotando() {
    for seed in [1u64, 42, 20_260_924, 999_983, 123_456_789] {
        let w = mundo(seed);
        let sueltos = floating_blocks(&w.grid);
        assert!(sueltos.is_empty(), "semilla {seed}: {sueltos:?}");
    }
}

#[test]
fn el_recorrido_completo_produce_imagenes_validas() {
    // Se recorre el guion entero a resolucion minima: ningun fotograma puede
    // salir negro, quemado o con valores no finitos.
    let w = mundo(SceneSpec::default().terrain.seed);
    let keys = tour_keys();
    let ajustes = ajustes(160, 90, 1);

    for i in 0..=12 {
        let t = i as f64 / 12.0;
        let mut c = tour_camera(&keys, t, 40.0);
        c.enforce_outside(w.grid.bounds(), 1.5);
        let r = render(&w, &c, &ajustes, None, None).expect("no deberia cancelarse");
        for tile in &r.framebuffer.tiles {
            for p in &tile.pixels {
                assert!(p.is_finite(), "fotograma {i} tiene valores no finitos");
            }
        }
        let e = estadisticas(&r.framebuffer.to_image(1.0));
        assert!(
            e.media > 20.0,
            "fotograma {i} demasiado oscuro: {}",
            e.media
        );
        assert!(e.media < 200.0, "fotograma {i} quemado: {}", e.media);
    }
}

#[test]
fn la_camara_nunca_entra_en_la_escena_durante_el_recorrido() {
    let w = mundo(SceneSpec::default().terrain.seed);
    let limites = w.grid.bounds();
    let keys = tour_keys();
    for i in 0..=400 {
        let mut c = tour_camera(&keys, i as f64 / 400.0, 40.0);
        c.enforce_outside(limites, 1.5);
        let p = c.position();
        assert!(
            !limites.contains(p),
            "la camara entro en la escena en {p:?}"
        );
        // Y tampoco puede quedar dentro de un bloque.
        let celda = w
            .grid
            .get(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        assert_eq!(celda, AIR);
    }
}

#[test]
fn el_recorrido_acelerado_coincide_con_la_referencia_en_la_escena_real() {
    // La comprobacion mas fuerte de la rejilla: sobre el diorama completo, no
    // sobre una escena de laboratorio.
    let w = mundo(SceneSpec::default().terrain.seed);
    let mut rng = Rng::new(20_260_928);
    let centro = Vec3 {
        x: 12.0,
        y: 10.0,
        z: 12.0,
    };
    let mut comparados = 0;

    for _ in 0..1500 {
        let origen = centro + rng.unit_vector() * 30.0;
        let objetivo = Vec3 {
            x: rng.range(0.0, 24.0),
            y: rng.range(0.0, 26.0),
            z: rng.range(0.0, 24.0),
        };
        let r = Ray::new(origen, objetivo - origen);
        let rapido = w.grid.hit(&r, Interval::positive(), AIR);
        let lento = w.grid.reference_hit(&r, Interval::positive(), AIR);
        match (rapido, lento) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                comparados += 1;
                assert!((a.t - b.t).abs() < 1e-6, "{} vs {}", a.t, b.t);
                assert_eq!(a.material, b.material);
                assert_eq!(a.face, b.face);
                assert_eq!(a.front_face, b.front_face);
            }
            (a, b) => panic!("discrepancia: {a:?} / {b:?}"),
        }
    }
    assert!(comparados > 900, "pocos impactos comparados: {comparados}");
}

#[test]
fn el_png_escrito_se_puede_releer_como_ppm_equivalente() {
    // Ida y vuelta por disco con los dos formatos del proyecto.
    let w = mundo(SceneSpec::default().terrain.seed);
    let mut c = Camera::initial();
    c.enforce_outside(w.grid.bounds(), 1.5);
    let r = render(&w, &c, &ajustes(160, 90, 1), None, None).unwrap();
    let img = r.framebuffer.to_image(1.0);

    let dir = std::env::temp_dir().join("abadia_integracion");
    std::fs::create_dir_all(&dir).unwrap();
    let ppm = dir.join("salida.ppm");
    let png = dir.join("salida.png");
    img.save(&ppm).unwrap();
    img.save(&png).unwrap();

    assert_eq!(Image::read_ppm(&ppm).unwrap(), img);
    // El PNG no se relee (no hay decodificador), pero si se comprueba que es un
    // fichero con firma valida y bastante mas pequeno que los datos crudos.
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
    assert!(
        bytes.len() < img.data.len(),
        "el PNG no comprimio: {} bytes para {} crudos",
        bytes.len(),
        img.data.len()
    );
    std::fs::remove_file(&ppm).ok();
    std::fs::remove_file(&png).ok();
}

#[test]
fn la_escena_cabe_en_la_rejilla_declarada() {
    let w = mundo(SceneSpec::default().terrain.seed);
    let [nx, ny, nz] = w.grid.dims();
    assert_eq!(nx, 24, "el terreno debe medir 24 celdas de lado");
    assert_eq!(nz, 24);
    assert!(nx * nz >= 16 * 16, "el minimo de la rubrica es 16 por 16");

    let mut cima = 0;
    for ([_, y, _], _) in w.grid.iter_solid() {
        cima = cima.max(y);
    }
    assert!(cima < ny, "algo sobresale de la rejilla: {cima} de {ny}");
    assert!(cima > 20, "la torre deberia levantarse de verdad: {cima}");
}

#[test]
fn una_rejilla_vacia_devuelve_solo_cielo() {
    // Control negativo: sin geometria, cada rayo tiene que acabar en el cubemap.
    let w = mundo(SceneSpec::default().terrain.seed);
    let vacio = World {
        grid: VoxelGrid::new(24, 26, 24),
        materials: {
            let (m, _) = abadia::material::MaterialSet::load(&assets());
            m
        },
        skybox: abadia::scene::load_skybox(&assets()).0,
        lighting: w.lighting,
    };
    let mut c = Camera::initial();
    c.enforce_outside(vacio.grid.bounds(), 1.5);
    let r = render(&vacio, &c, &ajustes(120, 68, 1), None, None).unwrap();
    let e = estadisticas(&r.framebuffer.to_image(1.0));
    assert!(e.media > 20.0, "el cielo no puede salir negro: {}", e.media);
    assert!(
        e.maximo - e.minimo < 130,
        "sin geometria no deberia haber tanto contraste"
    );
}
