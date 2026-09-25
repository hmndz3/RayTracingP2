//! Materiales del diorama: parametros fisicos, texturas y mapeo UV.
//!
//! Cada material lleva su propia textura, albedo, parametros especulares,
//! transparencia, reflectividad, indice de refraccion y emision. Los materiales
//! opacos declaran transparencia cero de forma explicita; el resto de valores se
//! eligen por comportamiento, no por conveniencia: el agua y el vidrio se
//! distinguen por su indice y por su absorcion, y el metal por ser conductor.

use crate::geometry::Hit;
use crate::math::{v3, Vec3};
use crate::ray::Medium;
use crate::texture::{Encoding, Filter, TextureId, TextureSet};
use std::path::Path;

/// Vacio. Es el unico identificador que no tiene material asociado.
pub const AIR: u16 = 0;
/// Piedra antigua de muros, arcos, columnas y contrafuertes.
pub const STONE_ANCIENT: u16 = 1;
/// Losa del camino, del atrio y del suelo interior.
pub const STONE_FLOOR: u16 = 2;
/// Escombro, fragmentos de muro caidos y lapidas.
pub const STONE_RUBBLE: u16 = 3;
/// Madera envejecida de vigas, pasarela y portones.
pub const WOOD_AGED: u16 = 4;
/// Capa superior del terreno: tierra con musgo.
pub const EARTH_MOSS: u16 = 5;
/// Tierra profunda, visible en el corte del terreno y en el fondo del estanque.
pub const EARTH_DARK: u16 = 6;
/// Agua del estanque.
pub const WATER: u16 = 7;
/// Vidrio de color del vitral.
pub const STAINED_GLASS: u16 = 8;
/// Bronce envejecido del escudo y de los herrajes.
pub const METAL_AGED: u16 = 9;
/// Material emisivo calido de los faroles.
pub const LANTERN: u16 = 10;
/// Material emisivo del altar, algo mas palido que el farol.
pub const ALTAR_CRYSTAL: u16 = 11;
/// Vegetacion discreta.
pub const FOLIAGE: u16 = 12;
/// Numero de identificadores, incluido el vacio.
pub const MATERIAL_COUNT: usize = 13;

/// Como se derivan las coordenadas de textura de un impacto.
#[derive(Debug, Clone, Copy)]
pub enum UvMode {
    /// La textura se repite una vez por bloque. Es el mapeo del diorama: mantiene
    /// el texel del mismo tamano en toda la escena y las juntas encajan entre
    /// bloques contiguos.
    PerBlock { scale: f64 },
    /// La textura se estira sobre un rectangulo del mundo. Lo usa el vitral, que
    /// es un unico dibujo repartido entre varios bloques: con el mapeo por bloque
    /// el rosetón se repetiria en cada cubo y se leeria como un azulejo.
    Window { origin: Vec3, su: f64, sv: f64 },
}

/// Parametros completos de un material.
#[derive(Debug, Clone)]
pub struct Material {
    pub name: &'static str,
    /// Textura de albedo; para los emisivos, tambien la de emision.
    pub albedo: TextureId,
    /// Mapa normal en espacio tangente, si lo tiene.
    pub normal_map: Option<TextureId>,
    /// Intensidad con la que se aplica el mapa normal.
    pub normal_strength: f64,
    /// Multiplicador de color sobre la textura.
    pub tint: Vec3,
    /// Peso del lobulo especular directo.
    pub specular: f64,
    /// Exponente de Phong del realce.
    pub shininess: f64,
    /// Reflectancia a incidencia normal, en `[0, 1]`. Para los dielectricos
    /// opacos es pequena; el metal la toma de su textura.
    pub reflectivity: f64,
    /// Verdadero si es conductor: sin componente difusa y con el reflejo tenido.
    pub metallic: bool,
    /// Fraccion de luz que el material transmite, en `[0, 1]`.
    pub transparency: f64,
    /// Indice de refraccion. Solo tiene efecto si transmite.
    pub ior: f64,
    /// Absorcion por unidad de distancia dentro del medio.
    pub absorption: Vec3,
    /// Si es mayor que cero, la absorcion se deduce del color de la textura en el
    /// punto de entrada, multiplicada por esta densidad. Es lo que hace que cada
    /// panel del vitral tina la luz que lo atraviesa con su propio color.
    pub absorption_from_texture: f64,
    /// Radiancia emitida, multiplicada por la textura.
    pub emission: Vec3,
    pub uv: UvMode,
}

impl Material {
    /// Verdadero si el material transmite luz.
    #[inline]
    pub fn is_transmissive(&self) -> bool {
        self.transparency > 1e-4
    }

    /// Verdadero si el material emite luz propia.
    #[inline]
    pub fn is_emissive(&self) -> bool {
        self.emission.max_component() > 1e-6
    }

    /// Coordenadas de textura de un impacto, segun el modo de mapeo.
    #[inline]
    pub fn uv_of(&self, hit: &Hit) -> (f64, f64) {
        match self.uv {
            UvMode::PerBlock { scale } => (hit.u * scale, hit.v * scale),
            UvMode::Window { origin, su, sv } => {
                let d = hit.point - origin;
                (d.dot(hit.tangent) / su, d.dot(hit.bitangent) / sv)
            }
        }
    }

    /// Medio que representa el interior de este material.
    pub fn medium(&self, tinte: Vec3) -> Medium {
        let absorption = if self.absorption_from_texture > 0.0 {
            // Ley de Beer-Lambert invertida: un panel que deja pasar el color `c`
            // tras una unidad de espesor tiene coeficiente `-ln(c)`.
            let c = tinte.max_elem(Vec3::splat(0.012)).min_elem(Vec3::ONE);
            v3(-c.x.ln(), -c.y.ln(), -c.z.ln()) * self.absorption_from_texture
        } else {
            self.absorption
        };
        Medium {
            ior: self.ior,
            absorption,
        }
    }
}

/// Coleccion de materiales con sus texturas ya cargadas.
#[derive(Debug)]
pub struct MaterialSet {
    materiales: Vec<Material>,
    pub textures: TextureSet,
}

/// Origen del vitral en el mundo y tamano del hueco que ocupa.
///
/// Se declara aqui porque tanto el material como el constructor de la escena
/// tienen que estar de acuerdo: el material estira el dibujo sobre este
/// rectangulo y la escena coloca los bloques de vidrio justo dentro de el.
pub const VITRAL_ORIGEN: Vec3 = v3(11.0, 16.0, 15.0);
/// Anchura del vitral en bloques.
pub const VITRAL_ANCHO: f64 = 5.0;
/// Altura del vitral en bloques.
pub const VITRAL_ALTO: f64 = 6.0;

impl MaterialSet {
    /// Carga las texturas de `assets/textures` y construye los materiales.
    ///
    /// Devuelve tambien la lista de avisos por recursos que no se pudieron leer:
    /// en ese caso el material usa un color liso y el render continua.
    pub fn load(assets: &Path) -> (MaterialSet, Vec<String>) {
        let dir = assets.join("textures");
        let mut tex = TextureSet::new();
        let mut avisos = Vec::new();

        let mut albedo = |tex: &mut TextureSet, nombre: &str, reserva: Vec3| -> TextureId {
            let (id, aviso) = tex.load(&dir, nombre, Encoding::Srgb, Filter::Nearest, reserva);
            if let Some(a) = aviso {
                avisos.push(a);
            }
            id
        };

        let t_stone = albedo(&mut tex, "stone_ancient", v3(0.32, 0.36, 0.42));
        let t_floor = albedo(&mut tex, "stone_floor", v3(0.30, 0.30, 0.31));
        let t_rubble = albedo(&mut tex, "stone_rubble", v3(0.35, 0.35, 0.36));
        let t_wood = albedo(&mut tex, "wood_aged", v3(0.14, 0.09, 0.05));
        let t_moss = albedo(&mut tex, "earth_moss", v3(0.12, 0.16, 0.09));
        let t_earth = albedo(&mut tex, "earth_dark", v3(0.07, 0.05, 0.04));
        let t_water = albedo(&mut tex, "water", v3(0.04, 0.10, 0.09));
        let t_glass = albedo(&mut tex, "stained_glass", v3(0.30, 0.20, 0.40));
        let t_metal = albedo(&mut tex, "metal_aged", v3(0.55, 0.45, 0.25));
        let t_lantern = albedo(&mut tex, "lantern_glow", v3(1.00, 0.60, 0.25));
        let t_altar = albedo(&mut tex, "altar_crystal", v3(1.00, 0.80, 0.45));
        let t_foliage = albedo(&mut tex, "foliage", v3(0.10, 0.17, 0.08));

        let normal = |tex: &mut TextureSet, nombre: &str| -> Option<TextureId> {
            let (id, aviso) = tex.load(
                &dir,
                &format!("{nombre}_n"),
                Encoding::Linear,
                Filter::Nearest,
                v3(0.5, 0.5, 1.0),
            );
            if aviso.is_some() {
                // Sin mapa normal el material sigue siendo valido: se sombrea con
                // la normal geometrica, que es exactamente el modo de comparacion.
                return None;
            }
            Some(id)
        };

        let n_stone = normal(&mut tex, "stone_ancient");
        let n_floor = normal(&mut tex, "stone_floor");
        let n_rubble = normal(&mut tex, "stone_rubble");
        let n_wood = normal(&mut tex, "wood_aged");
        let n_moss = normal(&mut tex, "earth_moss");
        let n_water = normal(&mut tex, "water");
        let n_metal = normal(&mut tex, "metal_aged");

        let opaco = UvMode::PerBlock { scale: 1.0 };

        // Plantilla de material opaco difuso, para no repetir los campos que casi
        // todos comparten.
        let base = Material {
            name: "",
            albedo: t_stone,
            normal_map: None,
            normal_strength: 1.0,
            tint: Vec3::ONE,
            specular: 0.04,
            shininess: 16.0,
            reflectivity: 0.02,
            metallic: false,
            transparency: 0.0,
            ior: 1.0,
            absorption: Vec3::ZERO,
            absorption_from_texture: 0.0,
            emission: Vec3::ZERO,
            uv: opaco,
        };

        let mut materiales = vec![
            // 0: vacio. Nunca se sombrea, pero ocupa la posicion para que el
            // identificador de material sea el indice directo.
            Material {
                name: "aire",
                ..base.clone()
            },
            // 1: piedra antigua. Rugosa, apenas reflectante, con juntas y relieve.
            Material {
                name: "piedra antigua",
                albedo: t_stone,
                normal_map: n_stone,
                normal_strength: 1.0,
                specular: 0.05,
                shininess: 18.0,
                reflectivity: 0.025,
                ..base.clone()
            },
            // 2: losa del camino, algo pulida por el paso.
            Material {
                name: "losa de camino",
                albedo: t_floor,
                normal_map: n_floor,
                normal_strength: 0.9,
                specular: 0.08,
                shininess: 28.0,
                reflectivity: 0.035,
                ..base.clone()
            },
            // 3: escombro y lapidas.
            Material {
                name: "escombro de piedra",
                albedo: t_rubble,
                normal_map: n_rubble,
                normal_strength: 1.0,
                specular: 0.04,
                shininess: 14.0,
                reflectivity: 0.02,
                ..base.clone()
            },
            // 4: madera envejecida, con brillo tenue y veta visible.
            Material {
                name: "madera envejecida",
                albedo: t_wood,
                normal_map: n_wood,
                normal_strength: 0.85,
                specular: 0.10,
                shininess: 34.0,
                reflectivity: 0.022,
                ..base.clone()
            },
            // 5: tierra con musgo, practicamente difusa.
            Material {
                name: "tierra con musgo",
                albedo: t_moss,
                normal_map: n_moss,
                normal_strength: 0.8,
                specular: 0.02,
                shininess: 8.0,
                reflectivity: 0.0,
                ..base.clone()
            },
            // 6: tierra profunda.
            Material {
                name: "tierra profunda",
                albedo: t_earth,
                specular: 0.015,
                shininess: 6.0,
                reflectivity: 0.0,
                ..base.clone()
            },
            // 7: agua. Transmite, refracta y refleja; absorbe sobre todo el rojo,
            // de ahi el tono azul verdoso al mirar al fondo del estanque.
            Material {
                name: "agua",
                albedo: t_water,
                normal_map: n_water,
                normal_strength: 0.55,
                specular: 0.45,
                shininess: 320.0,
                reflectivity: 1.0,
                transparency: 1.0,
                ior: 1.333,
                absorption: v3(0.46, 0.14, 0.11),
                ..base.clone()
            },
            // 8: vidrio de color. Indice mayor que el del agua y absorcion tomada
            // de la propia textura, asi que cada panel tine su luz transmitida.
            Material {
                name: "vidrio de color",
                albedo: t_glass,
                specular: 0.35,
                shininess: 260.0,
                reflectivity: 1.0,
                transparency: 1.0,
                ior: 1.52,
                absorption_from_texture: 2.6,
                uv: UvMode::Window {
                    origin: VITRAL_ORIGEN,
                    su: VITRAL_ANCHO,
                    sv: VITRAL_ALTO,
                },
                ..base.clone()
            },
            // 9: bronce envejecido. Conductor: sin difusa, reflejo tenido por su
            // propio color y realce ancho por la patina.
            Material {
                name: "bronce envejecido",
                albedo: t_metal,
                normal_map: n_metal,
                normal_strength: 0.5,
                specular: 0.9,
                shininess: 110.0,
                reflectivity: 0.86,
                metallic: true,
                ..base.clone()
            },
            // 10: farol. La emision multiplica la textura, que tiene nucleo
            // caliente y celosia oscura.
            Material {
                name: "farol emisivo",
                albedo: t_lantern,
                specular: 0.06,
                shininess: 20.0,
                reflectivity: 0.0,
                emission: v3(1.00, 0.62, 0.30) * 13.0,
                ..base.clone()
            },
            // 11: cristal del altar, algo mas palido y mas intenso.
            Material {
                name: "cristal del altar",
                albedo: t_altar,
                specular: 0.10,
                shininess: 40.0,
                reflectivity: 0.0,
                emission: v3(1.00, 0.80, 0.48) * 11.0,
                ..base.clone()
            },
            // 12: vegetacion.
            Material {
                name: "vegetacion",
                albedo: t_foliage,
                specular: 0.03,
                shininess: 10.0,
                reflectivity: 0.0,
                ..base
            },
        ];
        materiales.shrink_to_fit();
        debug_assert_eq!(materiales.len(), MATERIAL_COUNT);

        (
            MaterialSet {
                materiales,
                textures: tex,
            },
            avisos,
        )
    }

    #[inline]
    pub fn get(&self, id: u16) -> &Material {
        &self.materiales[id as usize]
    }

    pub fn len(&self) -> usize {
        self.materiales.len()
    }

    pub fn is_empty(&self) -> bool {
        self.materiales.is_empty()
    }

    /// Recorre los materiales con su identificador.
    pub fn iter(&self) -> impl Iterator<Item = (u16, &Material)> {
        self.materiales
            .iter()
            .enumerate()
            .map(|(i, m)| (i as u16, m))
    }

    /// Color de la textura de albedo en un impacto, ya en luz lineal.
    #[inline]
    pub fn albedo_at(&self, material: &Material, hit: &Hit) -> Vec3 {
        let (u, v) = material.uv_of(hit);
        self.textures
            .get(material.albedo)
            .sample(u, v)
            .mul_elem(material.tint)
    }

    /// Normal de sombreado en el espacio de la escena.
    ///
    /// La normal del mapa esta en espacio tangente; se lleva al espacio de la
    /// escena con la base de la cara impactada, que es derecha por construccion.
    /// `enabled` permite desactivar los mapas para poder comparar.
    pub fn shading_normal(&self, material: &Material, hit: &Hit, enabled: bool) -> Vec3 {
        let geometrica = hit.facing_normal();
        let Some(id) = material.normal_map else {
            return geometrica;
        };
        if !enabled || material.normal_strength <= 0.0 {
            return geometrica;
        }

        let (u, v) = material.uv_of(hit);
        let n_tangente = self.textures.get(id).sample_normal(u, v);
        // La fuerza interpola hacia la normal plana del espacio tangente.
        let n = v3(
            n_tangente.x * material.normal_strength,
            n_tangente.y * material.normal_strength,
            n_tangente.z,
        )
        .normalized();

        // La base se orienta con la cara vista: al mirar una superficie por
        // detras, la bitangente se invierte junto con la normal y la base sigue
        // siendo derecha.
        let (t, b, cara) = if hit.front_face {
            (hit.tangent, hit.bitangent, hit.normal)
        } else {
            (hit.tangent, -hit.bitangent, -hit.normal)
        };
        let mundo = (t * n.x + b * n.y + cara * n.z).normalized();

        // Salvaguarda: si el relieve inclinase la normal por debajo del horizonte
        // de la cara, la superficie se autosombrearia de forma imposible.
        if mundo.dot(geometrica) < 0.02 {
            geometrica
        } else {
            mundo
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Hit, FACE_NEG_Z, FACE_POS_Y};
    use std::path::PathBuf;

    fn assets() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
    }

    fn cargar() -> MaterialSet {
        let (set, avisos) = MaterialSet::load(&assets());
        assert!(avisos.is_empty(), "faltan recursos: {avisos:?}");
        set
    }

    #[test]
    fn se_cargan_los_doce_materiales_con_su_textura() {
        let set = cargar();
        assert_eq!(set.len(), MATERIAL_COUNT);
        for (id, m) in set.iter() {
            if id == AIR {
                continue;
            }
            assert!(!m.name.is_empty(), "material {id} sin nombre");
            let t = set.textures.get(m.albedo);
            assert!(t.width >= 32, "material {} sin textura real", m.name);
        }
    }

    #[test]
    fn el_encargo_pide_siete_materiales_y_estan_todos() {
        let set = cargar();
        // Piedra antigua: rugosa, poco reflectante, con mapa normal.
        let piedra = set.get(STONE_ANCIENT);
        assert!(piedra.normal_map.is_some());
        assert!(piedra.reflectivity < 0.05 && piedra.shininess < 40.0);
        assert_eq!(piedra.transparency, 0.0);

        // Madera envejecida: brillo tenue, mas que la piedra pero lejos del metal.
        let madera = set.get(WOOD_AGED);
        assert!(madera.specular > piedra.specular && madera.specular < 0.3);

        // Tierra con musgo: practicamente difusa.
        let tierra = set.get(EARTH_MOSS);
        assert!(tierra.specular < 0.05 && tierra.reflectivity == 0.0);

        // Agua: transparente, refractiva y reflectante.
        let agua = set.get(WATER);
        assert!(agua.is_transmissive() && agua.ior > 1.3 && agua.ior < 1.4);
        assert!(
            agua.absorption.x > agua.absorption.z,
            "el agua debe tirar a azul"
        );

        // Vidrio de color: distinto del agua en indice y en como tine.
        let vidrio = set.get(STAINED_GLASS);
        assert!(vidrio.is_transmissive());
        assert!(vidrio.ior > agua.ior + 0.1, "el vidrio debe refractar mas");
        assert!(vidrio.absorption_from_texture > 0.0);
        assert!(matches!(vidrio.uv, UvMode::Window { .. }));

        // Metal envejecido: conductor.
        let metal = set.get(METAL_AGED);
        assert!(metal.metallic && metal.reflectivity > 0.5);
        assert_eq!(metal.transparency, 0.0);

        // Emisivos.
        assert!(set.get(LANTERN).is_emissive());
        assert!(set.get(ALTAR_CRYSTAL).is_emissive());
        assert!(set.get(LANTERN).emission.x > set.get(LANTERN).emission.z);
    }

    #[test]
    fn los_opacos_declaran_transparencia_cero_y_los_transmisivos_un_indice_valido() {
        let set = cargar();
        for (id, m) in set.iter() {
            if id == AIR {
                continue;
            }
            assert!((0.0..=1.0).contains(&m.transparency), "{}", m.name);
            assert!((0.0..=1.0).contains(&m.reflectivity), "{}", m.name);
            if m.is_transmissive() {
                assert!(m.ior >= 1.0 && m.ior <= 2.5, "{} ior {}", m.name, m.ior);
                assert!(!m.metallic, "{} no puede ser conductor", m.name);
            } else {
                assert_eq!(m.transparency, 0.0, "{}", m.name);
            }
            if m.metallic {
                assert!(m.reflectivity > 0.4, "{} refleja muy poco", m.name);
            }
        }
    }

    #[test]
    fn solo_hay_dos_emisores_y_los_dos_son_calidos() {
        let set = cargar();
        let emisores: Vec<&str> = set
            .iter()
            .filter(|(_, m)| m.is_emissive())
            .map(|(_, m)| m.name)
            .collect();
        assert_eq!(emisores.len(), 2, "emisores: {emisores:?}");
        for (_, m) in set.iter().filter(|(_, m)| m.is_emissive()) {
            assert!(m.emission.x > m.emission.y && m.emission.y > m.emission.z);
            assert!(m.emission.x > 1.0, "la emision debe superar la unidad");
        }
    }

    #[test]
    fn el_medio_del_agua_absorbe_mas_el_rojo() {
        let set = cargar();
        let agua = set.get(WATER).medium(Vec3::ONE);
        assert!((agua.ior - 1.333).abs() < 1e-9);
        let t = agua.transmittance(2.0);
        assert!(t.x < t.z, "a dos metros el agua deberia verse azul verdosa");
        assert!(t.z > 0.5, "pero no tan opaca como para ocultar el fondo");
    }

    #[test]
    fn el_medio_del_vidrio_toma_el_color_del_panel() {
        let set = cargar();
        let vidrio = set.get(STAINED_GLASS);
        let ambar = vidrio.medium(v3(0.72, 0.33, 0.06));
        let azul = vidrio.medium(v3(0.06, 0.12, 0.47));
        // El panel ambar deja pasar el rojo; el azul deja pasar el azul.
        let ta = ambar.transmittance(0.4);
        let tz = azul.transmittance(0.4);
        assert!(ta.x > ta.z, "el panel ambar deberia transmitir calido");
        assert!(tz.z > tz.x, "el panel azul deberia transmitir frio");
    }

    #[test]
    fn un_panel_negro_no_provoca_absorcion_infinita() {
        let set = cargar();
        let m = set.get(STAINED_GLASS).medium(Vec3::ZERO);
        assert!(m.absorption.is_finite());
        assert!(m.transmittance(1.0).is_finite());
    }

    #[test]
    fn el_vitral_se_estira_sobre_su_hueco_y_no_se_repite_por_bloque() {
        let set = cargar();
        let vidrio = set.get(STAINED_GLASS);
        // Dos bloques distintos del vitral deben caer en zonas distintas del
        // dibujo: si el mapeo fuera por bloque, darian la misma UV.
        let a = Hit::from_face(
            1.0,
            v3(11.5, 15.5, 15.0),
            [11, 15, 15],
            FACE_NEG_Z,
            STAINED_GLASS,
            v3(0.0, 0.0, 1.0),
        );
        let b = Hit::from_face(
            1.0,
            v3(14.5, 12.5, 15.0),
            [14, 12, 15],
            FACE_NEG_Z,
            STAINED_GLASS,
            v3(0.0, 0.0, 1.0),
        );
        let (ua, va) = vidrio.uv_of(&a);
        let (ub, vb) = vidrio.uv_of(&b);
        assert!((ua - ub).abs() > 0.4 && (va - vb).abs() > 0.4);
        // Y el hueco completo se cubre exactamente una vez.
        for (u, v) in [(ua, va), (ub, vb)] {
            assert!((0.0..=1.0).contains(&u), "u fuera del hueco: {u}");
            assert!((0.0..=1.0).contains(&v), "v fuera del hueco: {v}");
        }
    }

    #[test]
    fn el_mapeo_por_bloque_repite_la_textura_en_cada_cubo() {
        let set = cargar();
        let piedra = set.get(STONE_ANCIENT);
        let a = Hit::from_face(
            1.0,
            v3(3.25, 5.75, 9.0),
            [3, 5, 9],
            FACE_NEG_Z,
            STONE_ANCIENT,
            v3(0.0, 0.0, 1.0),
        );
        let b = Hit::from_face(
            1.0,
            v3(8.25, 11.75, 9.0),
            [8, 11, 9],
            FACE_NEG_Z,
            STONE_ANCIENT,
            v3(0.0, 0.0, 1.0),
        );
        assert_eq!(piedra.uv_of(&a), piedra.uv_of(&b));
    }

    #[test]
    fn la_normal_de_sombreado_se_puede_desactivar() {
        let set = cargar();
        let piedra = set.get(STONE_ANCIENT);
        let h = Hit::from_face(
            1.0,
            v3(3.3, 5.0, 9.4),
            [3, 4, 9],
            FACE_POS_Y,
            STONE_ANCIENT,
            v3(0.2, -1.0, 0.1),
        );
        let con = set.shading_normal(piedra, &h, true);
        let sin = set.shading_normal(piedra, &h, false);
        assert_eq!(sin, h.facing_normal());
        assert!((con.length() - 1.0).abs() < 1e-9);
        assert!(con.dot(h.facing_normal()) > 0.0, "nunca debe invertirse");
    }

    #[test]
    fn el_mapa_normal_desvia_la_normal_en_las_juntas() {
        let set = cargar();
        let piedra = set.get(STONE_ANCIENT);
        // Se recorre una cara entera y se mide cuanto se desvia la normal.
        let mut maxima = 0.0f64;
        let mut media = 0.0;
        let n = 32;
        for i in 0..n {
            for j in 0..n {
                let p = v3(
                    3.0 + (i as f64 + 0.5) / n as f64,
                    5.0 + (j as f64 + 0.5) / n as f64,
                    9.0,
                );
                let h = Hit::from_face(
                    1.0,
                    p,
                    [3, 5, 9],
                    FACE_NEG_Z,
                    STONE_ANCIENT,
                    v3(0.0, 0.0, 1.0),
                );
                let desviacion = set
                    .shading_normal(piedra, &h, true)
                    .dot(h.facing_normal())
                    .clamp(-1.0, 1.0)
                    .acos();
                maxima = maxima.max(desviacion);
                media += desviacion;
            }
        }
        media /= (n * n) as f64;
        assert!(
            maxima.to_degrees() > 20.0,
            "el relieve no se nota: maximo {:.1} grados",
            maxima.to_degrees()
        );
        assert!(
            media.to_degrees() < 35.0,
            "el relieve domina toda la cara: media {:.1} grados",
            media.to_degrees()
        );
    }

    #[test]
    fn la_normal_es_coherente_en_las_seis_caras() {
        let set = cargar();
        let piedra = set.get(STONE_ANCIENT);
        for face in 0..6 {
            let normal = crate::geometry::face_normal(face);
            let p = v3(3.5, 5.5, 9.5) + normal * 0.5;
            let h = Hit::from_face(1.0, p, [3, 5, 9], face, STONE_ANCIENT, -normal);
            let n = set.shading_normal(piedra, &h, true);
            assert!(n.dot(normal) > 0.0, "cara {face}: la normal se invirtio");
            assert!((n.length() - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn el_albedo_se_lee_en_luz_lineal_y_es_plausible() {
        let set = cargar();
        for (id, m) in set.iter() {
            if id == AIR || m.is_emissive() {
                continue;
            }
            let h = Hit::from_face(
                1.0,
                v3(3.4, 5.6, 9.0),
                [3, 5, 9],
                FACE_NEG_Z,
                id,
                v3(0.0, 0.0, 1.0),
            );
            let a = set.albedo_at(m, &h);
            assert!(a.is_finite(), "{}", m.name);
            assert!(
                a.max_component() <= 1.0,
                "{} refleja mas de lo que recibe",
                m.name
            );
            assert!(a.max_component() > 0.0, "{} es negro absoluto", m.name);
        }
    }
}
