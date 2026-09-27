//! Iluminacion: luces direccionales, ambiente del entorno, oclusion de contacto,
//! sombras con transmitancia y muestreo explicito de los bloques emisivos.
//!
//! # Convenio de unidades
//!
//! Las luces se declaran por su irradiancia ya dividida entre `pi`, de modo que
//! la componente difusa es `albedo * cos(theta) * intensidad` sin mas factores.
//! Un emisor de area aporta `emision * omega / pi`, siendo `omega` el angulo
//! solido con el que se ve desde el punto sombreado, asi que las dos clases de
//! luz se suman en la misma escala.

use crate::acceleration::{Step, VoxelGrid};
use crate::geometry::{face_axis, face_normal, Hit, FACE_TANGENTS};
use crate::material::{Material, MaterialSet, AIR};
use crate::math::{v3, Onb, Rng, Vec3};
use crate::ray::{offset_ray, Interval, Medium, Ray};
use crate::skybox::{Skybox, SUN_DIR};

/// Direccion del relleno frio.
///
/// No coincide con la de la luna a proposito. La luna esta detras de la abadia y
/// desde ahi solo rozaria caras que la camara no ve; el relleno tiene que
/// levantar las caras visibles que el sol apenas alcanza, asi que viene de muy
/// arriba y ligeramente del lado del espectador. La luna sigue siendo su
/// justificacion visual en el cielo, no su direccion.
pub const FILL_DIR: Vec3 = v3(-0.3055, 0.8757, -0.4174);

/// Luz direccional: el sol poniente y el relleno frio de la luna.
#[derive(Debug, Clone, Copy)]
pub struct DirectionalLight {
    /// Direccion hacia la luz, normalizada.
    pub direction: Vec3,
    /// Intensidad ya dividida entre `pi`.
    pub color: Vec3,
    /// Si lanza rayo de sombra. El relleno no lo hace: su papel es levantar las
    /// caras que el sol no alcanza, y sombrearlo las devolveria a negro.
    pub casts_shadow: bool,
}

/// Agrupacion de celdas emisivas contiguas tratada como una sola luz de area.
///
/// Agrupar importa: los ocho bloques del altar iluminan como un cuerpo y no como
/// ocho fuentes puntuales solapadas, y el numero de luces a muestrear baja de
/// decenas a unas pocas.
#[derive(Debug, Clone, Copy)]
pub struct Emitter {
    pub center: Vec3,
    /// Radio de la esfera que envuelve el grupo.
    pub radius: f64,
    /// Radiancia del material emisor.
    pub emission: Vec3,
    /// Numero de celdas del grupo.
    pub cells: usize,
}

impl Emitter {
    /// Angulo solido con el que se ve el emisor desde un punto.
    ///
    /// Para una esfera de radio `r` a distancia `d` vale `2*pi*(1 - cos(a))`, con
    /// `sin(a) = r/d`. Dentro de la esfera se satura al hemisferio completo.
    #[inline]
    pub fn solid_angle(&self, from: Vec3) -> f64 {
        let d2 = (self.center - from).length_squared();
        let r2 = self.radius * self.radius;
        if d2 <= r2 * 1.0001 {
            return std::f64::consts::TAU;
        }
        let cos_max = (1.0 - r2 / d2).max(0.0).sqrt();
        std::f64::consts::TAU * (1.0 - cos_max)
    }

    /// Peso de importancia para elegir que emisores merece la pena muestrear.
    #[inline]
    pub fn importance(&self, from: Vec3) -> f64 {
        let d2 = (self.center - from).length_squared().max(1e-4);
        self.emission.luminance() * self.radius * self.radius / d2
    }
}

/// Parametros de iluminacion de la escena.
#[derive(Debug, Clone)]
pub struct Lighting {
    pub key: DirectionalLight,
    pub fill: DirectionalLight,
    /// Peso del entorno como luz ambiente.
    pub ambient: f64,
    pub emitters: Vec<Emitter>,
    /// Cuantos emisores se muestrean como maximo por punto sombreado.
    pub max_emitter_lights: usize,
    /// Muestras por emisor.
    pub emitter_samples: usize,
    /// Si se aplica la oclusion de contacto entre bloques.
    pub ambient_occlusion: bool,
}

impl Lighting {
    /// Iluminacion del anochecer: sol bajo ambar, relleno frio de luna y
    /// ambiente tomado del propio cubemap.
    pub fn dusk(emitters: Vec<Emitter>) -> Lighting {
        Lighting {
            key: DirectionalLight {
                direction: SUN_DIR.normalized(),
                color: v3(1.00, 0.600, 0.315) * 1.42,
                casts_shadow: true,
            },
            fill: DirectionalLight {
                direction: FILL_DIR.normalized(),
                color: v3(0.290, 0.380, 0.630) * 0.40,
                casts_shadow: false,
            },
            ambient: 0.62,
            emitters,
            max_emitter_lights: 4,
            emitter_samples: 2,
            ambient_occlusion: true,
        }
    }

    /// Recorre la rejilla agrupando las celdas emisivas contiguas.
    ///
    /// La union se hace por caras compartidas, con una pila explicita en lugar de
    /// recursion para que un grupo grande no pueda desbordar.
    pub fn collect_emitters(grid: &VoxelGrid, materials: &MaterialSet) -> Vec<Emitter> {
        let [nx, ny, nz] = grid.dims();
        let mut visitada = vec![false; grid.cell_count()];
        let indice = |i: i32, j: i32, k: i32| ((k * ny + j) * nx + i) as usize;
        let mut emisores = Vec::new();

        for ([i, j, k], mat) in grid.iter_solid() {
            if !materials.get(mat).is_emissive() || visitada[indice(i, j, k)] {
                continue;
            }
            let emission = materials.get(mat).emission;

            let mut pila = vec![[i, j, k]];
            visitada[indice(i, j, k)] = true;
            let mut celdas: Vec<[i32; 3]> = Vec::new();

            while let Some(c) = pila.pop() {
                celdas.push(c);
                for eje in 0..3 {
                    for paso in [-1i32, 1] {
                        let mut v = c;
                        v[eje] += paso;
                        if !grid.in_bounds(v[0], v[1], v[2]) {
                            continue;
                        }
                        let idx = indice(v[0], v[1], v[2]);
                        if visitada[idx] || grid.get(v[0], v[1], v[2]) != mat {
                            continue;
                        }
                        visitada[idx] = true;
                        pila.push(v);
                    }
                }
            }

            // Centro del grupo y radio que lo envuelve, medido a las esquinas.
            let mut centro = Vec3::ZERO;
            for c in &celdas {
                centro += v3(c[0] as f64 + 0.5, c[1] as f64 + 0.5, c[2] as f64 + 0.5);
            }
            centro = centro / celdas.len() as f64;
            let mut radio: f64 = 0.0;
            for c in &celdas {
                let esquina = v3(c[0] as f64 + 0.5, c[1] as f64 + 0.5, c[2] as f64 + 0.5);
                radio = radio.max((esquina - centro).length() + 0.5 * 3f64.sqrt());
            }

            emisores.push(Emitter {
                center: centro,
                radius: radio.max(0.55),
                emission,
                cells: celdas.len(),
            });
            let _ = nz;
        }

        emisores
    }
}

/// Transmitancia de un segmento hacia una luz.
///
/// Devuelve cuanta luz sobrevive. Un material opaco corta el rayo de inmediato;
/// uno transmisivo deja pasar su fraccion, atenuada por Beer-Lambert a lo largo
/// del tramo recorrido dentro de el. Gracias a eso el vitral proyecta luz de
/// color en vez de una sombra plana.
pub fn shadow_transmittance(
    grid: &VoxelGrid,
    materials: &MaterialSet,
    ray: &Ray,
    max_t: f64,
    entry: u16,
) -> Vec3 {
    let mut transmitancia = Vec3::ONE;
    // Tramo abierto dentro de un medio: parametro de entrada y su medio.
    let mut dentro: Option<(f64, Medium)> = None;

    grid.traverse(ray, Interval::new(1e-6, max_t), entry, |h| {
        let m = materials.get(h.material);

        if !m.is_transmissive() {
            // Los emisores tambien son cuerpos opacos: un farol tapa lo que hay
            // detras aunque brille.
            transmitancia = Vec3::ZERO;
            return Step::Stop;
        }

        if h.front_face {
            let tinte = materials.albedo_at(m, h);
            dentro = Some((h.t, m.medium(tinte)));
            // Perdida por la interfaz de entrada.
            transmitancia *= m.transparency * 0.92;
        } else if let Some((t0, medio)) = dentro.take() {
            transmitancia = transmitancia.mul_elem(medio.transmittance(h.t - t0));
            transmitancia *= 0.92;
        } else {
            // Salida sin entrada registrada: el rayo nacio dentro del medio.
            transmitancia *= 0.92;
        }

        if transmitancia.max_component() < 0.004 {
            transmitancia = Vec3::ZERO;
            return Step::Stop;
        }
        Step::Continue
    });

    transmitancia
}

/// Oclusion de contacto entre bloques vecinos.
///
/// Es la tecnica clasica de los mundos de voxels: la sombra de cada esquina de la
/// cara se deduce de si estan ocupados los dos bloques laterales y el diagonal, y
/// se interpola por las coordenadas de la cara. Cuesta ocho consultas a la
/// rejilla, sin un solo rayo, y es lo que hace que los arcos y los contrafuertes
/// se despeguen del muro en lugar de quedar pegados como una calcomania.
pub fn ambient_occlusion(grid: &VoxelGrid, materials: &MaterialSet, hit: &Hit) -> f64 {
    let cara = if hit.front_face {
        hit.face
    } else {
        // Al mirar la superficie por dentro, la cara relevante es la opuesta.
        hit.face ^ 1
    };
    let n = face_normal(cara);
    let (t, b) = FACE_TANGENTS[cara];

    let base = [
        hit.cell[0] + n.x as i32,
        hit.cell[1] + n.y as i32,
        hit.cell[2] + n.z as i32,
    ];
    let ocupada = |d: Vec3| -> bool {
        let c = [
            base[0] + d.x.round() as i32,
            base[1] + d.y.round() as i32,
            base[2] + d.z.round() as i32,
        ];
        let m = grid.get(c[0], c[1], c[2]);
        // Solo los cuerpos opacos ocluyen: el agua y el vidrio dejan pasar la luz.
        m != AIR && !materials.get(m).is_transmissive()
    };

    // Valor de esquina segun el criterio habitual: si los dos lados estan
    // ocupados la esquina queda totalmente encajonada.
    let esquina = |su: f64, sv: f64| -> f64 {
        let lado1 = ocupada(t * su);
        let lado2 = ocupada(b * sv);
        if lado1 && lado2 {
            return 0.0;
        }
        let diagonal = ocupada(t * su + b * sv);
        (3 - (lado1 as i32 + lado2 as i32 + diagonal as i32)) as f64 / 3.0
    };

    let (u, v) = (hit.u.clamp(0.0, 1.0), hit.v.clamp(0.0, 1.0));
    let a00 = esquina(-1.0, -1.0);
    let a10 = esquina(1.0, -1.0);
    let a01 = esquina(-1.0, 1.0);
    let a11 = esquina(1.0, 1.0);
    let arriba = a00 + (a10 - a00) * u;
    let abajo = a01 + (a11 - a01) * u;
    let factor = arriba + (abajo - arriba) * v;

    // Nunca llega a cero: una esquina totalmente encajonada sigue recibiendo algo
    // de luz rebotada, y apagarla del todo produciria manchas negras.
    0.40 + 0.60 * factor
}

/// Resultado del calculo de luz directa sobre un punto.
#[derive(Debug, Clone, Copy)]
pub struct DirectLight {
    /// Irradiancia difusa, ya dividida entre `pi`.
    pub diffuse: Vec3,
    /// Termino especular, sin multiplicar por el peso del material.
    pub specular: Vec3,
}

/// Contexto compacto que necesitan las rutinas de iluminacion.
pub struct LightContext<'a> {
    pub grid: &'a VoxelGrid,
    pub materials: &'a MaterialSet,
    pub skybox: &'a Skybox,
    pub lighting: &'a Lighting,
}

/// Luz directa que llega a un punto: las dos direccionales mas los emisores
/// seleccionados por importancia.
///
/// `view` es la direccion hacia la camara, `normal` la normal de sombreado ya con
/// el mapa aplicado y `medium` el material en el que viaja el rayo, que hace falta
/// para que un rayo de sombra lanzado desde dentro del agua no vea una interfaz
/// inexistente al cruzar la celda contigua.
#[allow(clippy::too_many_arguments)]
pub fn direct_light(
    ctx: &LightContext,
    hit: &Hit,
    normal: Vec3,
    view: Vec3,
    material: &Material,
    medium_material: u16,
    rng: &mut Rng,
) -> DirectLight {
    let mut diffuse = Vec3::ZERO;
    let mut specular = Vec3::ZERO;

    let mut aportar = |luz_dir: Vec3, color: Vec3, transmitancia: Vec3| {
        let cos = normal.dot(luz_dir);
        if cos <= 0.0 {
            return;
        }
        let recibida = color.mul_elem(transmitancia);
        diffuse += recibida * cos;
        if material.specular > 0.0 {
            // Blinn-Phong: el vector medio evita recalcular la reflexion y da un
            // realce mas estable en incidencia rasante.
            let medio = (luz_dir + view).normalized();
            let brillo = normal.dot(medio).max(0.0).powf(material.shininess);
            specular += recibida * brillo;
        }
    };

    // Luz principal, con sombra.
    let key = ctx.lighting.key;
    if normal.dot(key.direction) > 0.0 {
        let t = if key.casts_shadow {
            let r = offset_ray(hit.point, hit.normal, key.direction);
            shadow_transmittance(ctx.grid, ctx.materials, &r, 200.0, medium_material)
        } else {
            Vec3::ONE
        };
        if t.max_component() > 0.0 {
            aportar(key.direction, key.color, t);
        }
    }

    // Relleno frio, sin sombra.
    let fill = ctx.lighting.fill;
    aportar(fill.direction, fill.color, Vec3::ONE);

    // Emisores: se ordenan por importancia y solo se muestrean los mas fuertes.
    let n_luces = ctx
        .lighting
        .max_emitter_lights
        .min(ctx.lighting.emitters.len());
    if n_luces > 0 {
        let mut mejores: Vec<(f64, usize)> = Vec::with_capacity(n_luces + 1);
        for (i, e) in ctx.lighting.emitters.iter().enumerate() {
            let w = e.importance(hit.point);
            // Umbral de corte: por debajo de esto el emisor no llega a mover un
            // solo nivel de los 256 de la imagen final.
            if w < 2.0e-4 {
                continue;
            }
            if mejores.len() < n_luces {
                mejores.push((w, i));
                mejores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            } else if w > mejores[n_luces - 1].0 {
                mejores[n_luces - 1] = (w, i);
                mejores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            }
        }

        let muestras = ctx.lighting.emitter_samples.max(1);
        for (_, idx) in mejores {
            let e = ctx.lighting.emitters[idx];
            let hacia = e.center - hit.point;
            let distancia = hacia.length();
            if distancia < 1e-6 {
                continue;
            }
            let omega = e.solid_angle(hit.point);
            let intensidad = e.emission * (omega / std::f64::consts::PI);

            // El muestreo reparte el angulo solido entre las muestras: cada una
            // apunta a un punto distinto de la esfera que envuelve el grupo, con
            // estratificacion sobre el disco visible.
            let base = Onb::from_normal(hacia / distancia);
            let radio_aparente = (e.radius / distancia).min(1.0);
            for s in 0..muestras {
                let (r1, r2) = estratificar(s, muestras, rng);
                let radio = radio_aparente * r1.sqrt();
                let angulo = std::f64::consts::TAU * r2;
                let desvio =
                    base.tangent * (radio * angulo.cos()) + base.bitangent * (radio * angulo.sin());
                let dir = (base.normal + desvio).normalized();

                let cos = normal.dot(dir);
                if cos <= 0.0 {
                    continue;
                }
                let r = offset_ray(hit.point, hit.normal, dir);
                // Se detiene justo antes del propio emisor, para que su superficie
                // no se tape a si misma.
                let alcance = (distancia - e.radius * 0.5).max(1e-3);
                let t = shadow_transmittance(ctx.grid, ctx.materials, &r, alcance, medium_material);
                if t.max_component() <= 0.0 {
                    continue;
                }
                let peso = 1.0 / muestras as f64;
                aportar(dir, intensidad * peso, t);
            }
        }
    }

    DirectLight { diffuse, specular }
}

/// Muestra estratificada dentro de la celda `i` de una particion de `n`.
///
/// Con pocas muestras la estratificacion importa mas que el generador: reparte el
/// disco en franjas y evita que las dos muestras de un farol caigan juntas, que es
/// lo que produce el granulado en los bordes de sombra.
#[inline]
fn estratificar(i: usize, n: usize, rng: &mut Rng) -> (f64, f64) {
    let r1 = (i as f64 + rng.next_f64()) / n as f64;
    let r2 = rng.next_f64();
    (r1, r2)
}

/// Luz ambiente procedente del entorno, modulada por la oclusion de contacto.
pub fn ambient_light(ctx: &LightContext, hit: &Hit, normal: Vec3) -> Vec3 {
    let entorno = ctx.skybox.sample(normal);
    let oclusion = if ctx.lighting.ambient_occlusion {
        ambient_occlusion(ctx.grid, ctx.materials, hit)
    } else {
        1.0
    };
    entorno * (ctx.lighting.ambient * oclusion)
}

/// Eje dominante de una cara, expuesto para las pruebas de coherencia.
#[inline]
pub fn face_major_axis(face: usize) -> usize {
    face_axis(face)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::{ALTAR_CRYSTAL, LANTERN, STONE_ANCIENT, WATER};
    use crate::skybox::Skybox;
    use std::path::PathBuf;

    fn materiales() -> MaterialSet {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let (set, avisos) = MaterialSet::load(&assets);
        assert!(avisos.is_empty(), "faltan recursos: {avisos:?}");
        set
    }

    #[test]
    fn los_emisores_contiguos_se_agrupan_en_una_sola_luz() {
        let m = materiales();
        let mut g = VoxelGrid::new(16, 16, 16);
        // Un altar de dos por dos por uno.
        for i in 4..6 {
            for k in 4..6 {
                g.set(i, 8, k, ALTAR_CRYSTAL);
            }
        }
        // Y un farol suelto, lejos.
        g.set(12, 10, 12, LANTERN);

        let e = Lighting::collect_emitters(&g, &m);
        assert_eq!(e.len(), 2, "deberian ser dos grupos: {e:?}");
        let altar = e.iter().find(|e| e.cells == 4).expect("grupo del altar");
        assert!((altar.center - v3(5.0, 8.5, 5.0)).length() < 1e-9);
        assert!(
            altar.radius > 0.9,
            "el radio debe envolver las cuatro celdas"
        );
        let farol = e.iter().find(|e| e.cells == 1).expect("farol suelto");
        assert!((farol.center - v3(12.5, 10.5, 12.5)).length() < 1e-9);
    }

    #[test]
    fn los_materiales_no_emisivos_no_generan_luces() {
        let m = materiales();
        let mut g = VoxelGrid::new(8, 8, 8);
        for i in 0..8 {
            g.set(i, 0, 0, STONE_ANCIENT);
        }
        assert!(Lighting::collect_emitters(&g, &m).is_empty());
    }

    #[test]
    fn dos_grupos_de_material_distinto_no_se_funden() {
        let m = materiales();
        let mut g = VoxelGrid::new(8, 8, 8);
        g.set(3, 3, 3, LANTERN);
        g.set(4, 3, 3, ALTAR_CRYSTAL);
        let e = Lighting::collect_emitters(&g, &m);
        assert_eq!(e.len(), 2, "materiales distintos son luces distintas");
    }

    #[test]
    fn el_angulo_solido_decrece_con_la_distancia() {
        let e = Emitter {
            center: Vec3::ZERO,
            radius: 0.5,
            emission: Vec3::ONE,
            cells: 1,
        };
        let cerca = e.solid_angle(v3(0.0, 2.0, 0.0));
        let lejos = e.solid_angle(v3(0.0, 20.0, 0.0));
        assert!(cerca > lejos * 10.0, "deberia caer con el cuadrado");
        assert!(lejos > 0.0);
        // A gran distancia tiende al area aparente: pi*r^2/d^2.
        let esperado = std::f64::consts::PI * 0.25 / 400.0;
        assert!((lejos - esperado).abs() / esperado < 0.01);
        // Dentro de la esfera se satura al hemisferio.
        assert!((e.solid_angle(Vec3::ZERO) - std::f64::consts::TAU).abs() < 1e-9);
    }

    #[test]
    fn la_importancia_ordena_por_cercania_y_potencia() {
        let debil = Emitter {
            center: v3(0.0, 0.0, 0.0),
            radius: 0.5,
            emission: Vec3::ONE,
            cells: 1,
        };
        let fuerte = Emitter {
            center: v3(0.0, 0.0, 0.0),
            radius: 0.5,
            emission: Vec3::splat(10.0),
            cells: 1,
        };
        let p = v3(3.0, 0.0, 0.0);
        assert!(fuerte.importance(p) > debil.importance(p) * 9.0);
        assert!(debil.importance(v3(1.0, 0.0, 0.0)) > debil.importance(v3(9.0, 0.0, 0.0)));
    }

    #[test]
    fn un_muro_opaco_corta_la_luz_por_completo() {
        let m = materiales();
        let mut g = VoxelGrid::new(16, 16, 16);
        for j in 0..16 {
            for k in 0..16 {
                g.set(8, j, k, STONE_ANCIENT);
            }
        }
        let r = Ray::new(v3(2.5, 8.5, 8.5), v3(1.0, 0.0, 0.0));
        let t = shadow_transmittance(&g, &m, &r, 20.0, AIR);
        assert_eq!(t, Vec3::ZERO);
    }

    #[test]
    fn sin_obstaculos_la_luz_llega_entera() {
        let m = materiales();
        let g = VoxelGrid::new(16, 16, 16);
        let r = Ray::new(v3(2.5, 8.5, 8.5), v3(1.0, 0.0, 0.0));
        assert_eq!(shadow_transmittance(&g, &m, &r, 20.0, AIR), Vec3::ONE);
    }

    #[test]
    fn el_agua_deja_pasar_luz_atenuada_y_tenida() {
        let m = materiales();
        let mut g = VoxelGrid::new(16, 16, 16);
        for i in 6..10 {
            g.set(i, 8, 8, WATER);
        }
        let r = Ray::new(v3(2.5, 8.5, 8.5), v3(1.0, 0.0, 0.0));
        let t = shadow_transmittance(&g, &m, &r, 20.0, AIR);
        assert!(t.max_component() > 0.0, "el agua no deberia ser opaca");
        assert!(t.max_component() < 1.0, "pero si atenuar");
        assert!(t.x < t.z, "y absorber mas el rojo: {t:?}");
    }

    #[test]
    fn un_farol_tapa_lo_que_tiene_detras() {
        let m = materiales();
        let mut g = VoxelGrid::new(16, 16, 16);
        g.set(8, 8, 8, LANTERN);
        let r = Ray::new(v3(2.5, 8.5, 8.5), v3(1.0, 0.0, 0.0));
        assert_eq!(shadow_transmittance(&g, &m, &r, 20.0, AIR), Vec3::ZERO);
    }

    #[test]
    fn la_oclusion_de_contacto_oscurece_los_rincones() {
        let m = materiales();
        let mut g = VoxelGrid::new(16, 16, 16);
        // Suelo llano.
        for i in 0..16 {
            for k in 0..16 {
                g.set(i, 4, k, STONE_ANCIENT);
            }
        }
        let abierto = Hit::from_face(
            1.0,
            v3(8.5, 5.0, 8.5),
            [8, 4, 8],
            crate::geometry::FACE_POS_Y,
            STONE_ANCIENT,
            v3(0.0, -1.0, 0.0),
        );
        let libre = ambient_occlusion(&g, &m, &abierto);
        assert!((libre - 1.0).abs() < 1e-9, "el llano no deberia ocluirse");

        // Se levanta un muro pegado y se mide junto a el.
        for i in 0..16 {
            for j in 5..9 {
                g.set(i, j, 9, STONE_ANCIENT);
            }
        }
        let rincon = Hit::from_face(
            1.0,
            v3(8.5, 5.0, 8.98),
            [8, 4, 8],
            crate::geometry::FACE_POS_Y,
            STONE_ANCIENT,
            v3(0.0, -1.0, 0.0),
        );
        let ocluido = ambient_occlusion(&g, &m, &rincon);
        assert!(
            ocluido < libre * 0.9,
            "el rincon deberia oscurecerse: {ocluido}"
        );
        assert!(ocluido >= 0.40, "nunca debe llegar a negro: {ocluido}");
    }

    #[test]
    fn la_oclusion_de_contacto_es_continua_sobre_la_cara() {
        let m = materiales();
        let mut g = VoxelGrid::new(16, 16, 16);
        for i in 0..16 {
            for k in 0..16 {
                g.set(i, 4, k, STONE_ANCIENT);
            }
        }
        for j in 5..8 {
            g.set(9, j, 8, STONE_ANCIENT);
        }
        let mut anterior: Option<f64> = None;
        for s in 0..=40 {
            let u = s as f64 / 40.0;
            let h = Hit::from_face(
                1.0,
                v3(8.0 + u, 5.0, 8.5),
                [8, 4, 8],
                crate::geometry::FACE_POS_Y,
                STONE_ANCIENT,
                v3(0.0, -1.0, 0.0),
            );
            let ao = ambient_occlusion(&g, &m, &h);
            assert!((0.40..=1.0).contains(&ao));
            if let Some(p) = anterior {
                assert!((ao - p).abs() < 0.08, "salto brusco de oclusion");
            }
            anterior = Some(ao);
        }
    }

    #[test]
    fn el_agua_no_ocluye_el_contacto() {
        let m = materiales();
        let mut g = VoxelGrid::new(16, 16, 16);
        for i in 0..16 {
            for k in 0..16 {
                g.set(i, 4, k, STONE_ANCIENT);
            }
        }
        for i in 0..16 {
            for k in 0..16 {
                g.set(i, 5, k, WATER);
            }
        }
        let h = Hit::from_face(
            1.0,
            v3(8.5, 5.0, 8.5),
            [8, 4, 8],
            crate::geometry::FACE_POS_Y,
            STONE_ANCIENT,
            v3(0.0, -1.0, 0.0),
        );
        assert!((ambient_occlusion(&g, &m, &h) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn la_luz_directa_ilumina_lo_expuesto_y_deja_a_oscuras_lo_tapado() {
        let m = materiales();
        let sb = Skybox::fallback();
        let mut g = VoxelGrid::new(24, 24, 24);
        for i in 0..24 {
            for k in 0..24 {
                g.set(i, 4, k, STONE_ANCIENT);
            }
        }
        let luces = Lighting::dusk(Vec::new());
        let ctx = LightContext {
            grid: &g,
            materials: &m,
            skybox: &sb,
            lighting: &luces,
        };
        let mat = m.get(STONE_ANCIENT);
        let mut rng = Rng::new(1);

        let h = Hit::from_face(
            1.0,
            v3(8.5, 5.0, 8.5),
            [8, 4, 8],
            crate::geometry::FACE_POS_Y,
            STONE_ANCIENT,
            v3(0.0, -1.0, 0.0),
        );
        let arriba = v3(0.0, 1.0, 0.0);
        let vista = v3(0.0, 1.0, 0.0);
        let expuesto = direct_light(&ctx, &h, arriba, vista, mat, AIR, &mut rng);
        assert!(
            expuesto.diffuse.luminance() > 0.05,
            "{:?}",
            expuesto.diffuse
        );
        // El sol es calido: el canal rojo debe dominar.
        assert!(expuesto.diffuse.x > expuesto.diffuse.z);

        // Bajo un techo bajo, la luz principal queda cortada y solo queda el
        // relleno. El techo va justo encima porque el sol esta a catorce grados:
        // uno alto dejaria escapar el rayo por el borde de la rejilla antes de
        // alcanzarlo.
        for i in 0..24 {
            for k in 0..24 {
                g.set(i, 6, k, STONE_ANCIENT);
                g.set(i, 7, k, STONE_ANCIENT);
            }
        }
        let ctx2 = LightContext {
            grid: &g,
            materials: &m,
            skybox: &sb,
            lighting: &luces,
        };
        let tapado = direct_light(&ctx2, &h, arriba, vista, mat, AIR, &mut rng);
        assert!(tapado.diffuse.luminance() < expuesto.diffuse.luminance() * 0.6);
        assert!(
            tapado.diffuse.luminance() > 0.0,
            "el relleno no debe apagarse"
        );
    }

    #[test]
    fn un_emisor_cercano_ilumina_mas_que_uno_lejano() {
        let m = materiales();
        let sb = Skybox::fallback();
        let mut g = VoxelGrid::new(32, 32, 32);
        for i in 0..32 {
            for k in 0..32 {
                g.set(i, 4, k, STONE_ANCIENT);
            }
        }
        g.set(8, 7, 8, LANTERN);
        let emisores = Lighting::collect_emitters(&g, &m);
        assert_eq!(emisores.len(), 1);
        let mut luces = Lighting::dusk(emisores);
        // Se apagan las direccionales para medir solo el emisor.
        luces.key.color = Vec3::ZERO;
        luces.fill.color = Vec3::ZERO;
        luces.emitter_samples = 8;

        let ctx = LightContext {
            grid: &g,
            materials: &m,
            skybox: &sb,
            lighting: &luces,
        };
        let mat = m.get(STONE_ANCIENT);
        let mut rng = Rng::new(42);
        let medir = |x: f64, rng: &mut Rng| {
            let h = Hit::from_face(
                1.0,
                v3(x, 5.0, 8.5),
                [x.floor() as i32, 4, 8],
                crate::geometry::FACE_POS_Y,
                STONE_ANCIENT,
                v3(0.0, -1.0, 0.0),
            );
            direct_light(
                &ctx,
                &h,
                v3(0.0, 1.0, 0.0),
                v3(0.0, 1.0, 0.0),
                mat,
                AIR,
                rng,
            )
            .diffuse
            .luminance()
        };
        let cerca = medir(8.5, &mut rng);
        let medio = medir(11.5, &mut rng);
        let lejos = medir(20.5, &mut rng);
        assert!(cerca > medio && medio > lejos, "{cerca} {medio} {lejos}");
        assert!(cerca > 0.02, "el farol no ilumina nada: {cerca}");
    }

    #[test]
    fn el_emisor_no_atraviesa_un_muro() {
        let m = materiales();
        let sb = Skybox::fallback();
        let mut g = VoxelGrid::new(32, 32, 32);
        for i in 0..32 {
            for k in 0..32 {
                g.set(i, 4, k, STONE_ANCIENT);
            }
        }
        g.set(8, 7, 8, LANTERN);
        // Muro entre el farol y el punto medido.
        for j in 5..12 {
            for k in 0..32 {
                g.set(14, j, k, STONE_ANCIENT);
            }
        }
        let mut luces = Lighting::dusk(Lighting::collect_emitters(&g, &m));
        luces.key.color = Vec3::ZERO;
        luces.fill.color = Vec3::ZERO;
        luces.emitter_samples = 6;
        let ctx = LightContext {
            grid: &g,
            materials: &m,
            skybox: &sb,
            lighting: &luces,
        };
        let mat = m.get(STONE_ANCIENT);
        let mut rng = Rng::new(7);
        let h = Hit::from_face(
            1.0,
            v3(18.5, 5.0, 8.5),
            [18, 4, 8],
            crate::geometry::FACE_POS_Y,
            STONE_ANCIENT,
            v3(0.0, -1.0, 0.0),
        );
        let d = direct_light(
            &ctx,
            &h,
            v3(0.0, 1.0, 0.0),
            v3(0.0, 1.0, 0.0),
            mat,
            AIR,
            &mut rng,
        );
        assert!(
            d.diffuse.luminance() < 1e-6,
            "la luz atraviesa el muro: {d:?}"
        );
    }

    #[test]
    fn muchos_emisores_no_disparan_el_coste_por_punto() {
        let m = materiales();
        let sb = Skybox::fallback();
        let mut g = VoxelGrid::new(48, 16, 48);
        for i in 0..48 {
            for k in 0..48 {
                g.set(i, 4, k, STONE_ANCIENT);
            }
        }
        // Cuarenta y nueve faroles repartidos.
        for i in 0..7 {
            for k in 0..7 {
                g.set(2 + i * 6, 7, 2 + k * 6, LANTERN);
            }
        }
        let emisores = Lighting::collect_emitters(&g, &m);
        assert_eq!(emisores.len(), 49);
        let luces = Lighting::dusk(emisores);
        assert_eq!(luces.max_emitter_lights, 4, "el tope debe acotar el coste");

        let ctx = LightContext {
            grid: &g,
            materials: &m,
            skybox: &sb,
            lighting: &luces,
        };
        let mat = m.get(STONE_ANCIENT);
        let mut rng = Rng::new(3);
        let h = Hit::from_face(
            1.0,
            v3(24.5, 5.0, 24.5),
            [24, 4, 24],
            crate::geometry::FACE_POS_Y,
            STONE_ANCIENT,
            v3(0.0, -1.0, 0.0),
        );
        let d = direct_light(
            &ctx,
            &h,
            v3(0.0, 1.0, 0.0),
            v3(0.0, 1.0, 0.0),
            mat,
            AIR,
            &mut rng,
        );
        assert!(d.diffuse.is_finite() && d.diffuse.luminance() > 0.0);
    }

    #[test]
    fn la_luz_ambiente_nunca_deja_una_cara_en_negro() {
        let m = materiales();
        let sb = Skybox::fallback();
        let mut g = VoxelGrid::new(16, 16, 16);
        for i in 0..16 {
            for k in 0..16 {
                g.set(i, 4, k, STONE_ANCIENT);
            }
        }
        let luces = Lighting::dusk(Vec::new());
        let ctx = LightContext {
            grid: &g,
            materials: &m,
            skybox: &sb,
            lighting: &luces,
        };
        for face in 0..6 {
            let n = face_normal(face);
            let h = Hit::from_face(
                1.0,
                v3(8.5, 5.0, 8.5) + n * 0.5,
                [8, 4, 8],
                face,
                STONE_ANCIENT,
                -n,
            );
            let a = ambient_light(&ctx, &h, n);
            assert!(a.luminance() > 0.0, "cara {face} sin ambiente");
            assert!(a.is_finite());
        }
    }
}
