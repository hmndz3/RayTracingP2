//! Camara orbital: gira alrededor de un punto de interes y se acerca o se aleja
//! de el, con limites que impiden atravesar la escena o invertirse en los polos.

use crate::geometry::Aabb;
use crate::math::{degrees_to_radians, lerp, smootherstep, v3, Vec3};
use crate::ray::{Interval, Ray};

/// Limites del orbitador.
///
/// La inclinacion nunca alcanza los 90 grados, asi que el vector hacia la camara
/// jamas es paralelo al eje vertical y la base de la vista no puede degenerar:
/// esa es la razon por la que la imagen no se voltea al pasar por el cenit. El
/// tope inferior de distancia es solo un suelo absoluto; que la camara no
/// atraviese la geometria lo garantiza [`Camera::enforce_outside`], que empuja la
/// distancia hasta que el punto de vista queda fuera del volumen del diorama.
pub const PITCH_MIN: f64 = 4.0;
pub const PITCH_MAX: f64 = 80.0;
pub const DISTANCE_MIN: f64 = 14.0;
pub const DISTANCE_MAX: f64 = 70.0;

/// Camara orbital definida en coordenadas esfericas alrededor de `target`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Punto al que mira la camara.
    pub target: Vec3,
    /// Azimut en grados. Cero coloca la camara sobre el eje `+Z` del objetivo.
    pub yaw: f64,
    /// Elevacion en grados sobre el plano horizontal.
    pub pitch: f64,
    /// Distancia al objetivo.
    pub distance: f64,
    /// Campo de vision vertical en grados.
    pub vfov: f64,
}

/// Base de la vista ya resuelta, mas los factores del plano de imagen.
///
/// Se calcula una sola vez por fotograma y se comparte entre todos los hilos, de
/// modo que generar un rayo primario cuesta dos multiplicaciones y una suma.
#[derive(Debug, Clone, Copy)]
pub struct CameraBasis {
    pub origin: Vec3,
    /// Direccion hacia el centro de la imagen.
    pub forward: Vec3,
    /// Eje horizontal de la imagen, escalado a media anchura del plano.
    pub half_u: Vec3,
    /// Eje vertical de la imagen, escalado a media altura del plano.
    pub half_v: Vec3,
    pub width: f64,
    pub height: f64,
}

impl Camera {
    /// Vista inicial del diorama: la abadia al fondo, el estanque delante y los
    /// faroles encendidos, todo dentro del encuadre desde el primer fotograma.
    pub fn initial() -> Camera {
        Camera {
            target: v3(11.0, 9.5, 11.5),
            yaw: -147.0,
            pitch: 16.0,
            distance: 37.0,
            vfov: 40.0,
        }
    }

    /// Aplica los limites de inclinacion y distancia.
    pub fn clamp(&mut self) {
        self.pitch = self.pitch.clamp(PITCH_MIN, PITCH_MAX);
        self.distance = self.distance.clamp(DISTANCE_MIN, DISTANCE_MAX);
        // El azimut se normaliza para que girar sin parar no acumule error.
        while self.yaw <= -180.0 {
            self.yaw += 360.0;
        }
        while self.yaw > 180.0 {
            self.yaw -= 360.0;
        }
    }

    /// Gira en azimut y elevacion, en grados.
    pub fn orbit(&mut self, delta_yaw: f64, delta_pitch: f64) {
        self.yaw += delta_yaw;
        self.pitch += delta_pitch;
        self.clamp();
    }

    /// Acerca o aleja la camara de forma multiplicativa, para que el paso se
    /// sienta igual de grande a cualquier distancia.
    pub fn zoom(&mut self, factor: f64) {
        self.distance *= factor;
        self.clamp();
    }

    /// Empuja la camara hasta que queda fuera de `bounds`, con un margen.
    ///
    /// Se resuelve con el mismo test de rebanadas que usa el trazador: se lanza un
    /// rayo desde el objetivo en la direccion en la que esta la camara y se toma
    /// la distancia a la que ese rayo abandona la caja. Asi el orbitador puede
    /// acercarse mucho a un detalle sin que ningun encuadre acabe dentro de un
    /// muro, y sin imponer una distancia minima innecesariamente grande.
    pub fn enforce_outside(&mut self, bounds: Aabb, margin: f64) {
        let (sy, cy) = degrees_to_radians(self.yaw).sin_cos();
        let (sp, cp) = degrees_to_radians(self.pitch).sin_cos();
        let dir = v3(cp * sy, sp, cp * cy);
        let salida = Ray::new(self.target, dir);
        if let Some(slab) = bounds.hit(&salida, Interval::new(0.0, f64::INFINITY)) {
            let minima = (slab.t_exit + margin).min(DISTANCE_MAX);
            if self.distance < minima {
                self.distance = minima;
            }
        }
        self.clamp();
    }

    /// Posicion de la camara en el espacio de la escena.
    pub fn position(&self) -> Vec3 {
        let (sy, cy) = degrees_to_radians(self.yaw).sin_cos();
        let (sp, cp) = degrees_to_radians(self.pitch).sin_cos();
        self.target + v3(cp * sy, sp, cp * cy) * self.distance
    }

    /// Resuelve la base de la vista para una resolucion concreta.
    pub fn basis(&self, width: usize, height: usize) -> CameraBasis {
        let origin = self.position();
        // `back` apunta de la escena hacia la camara; es el eje w de la base.
        let back = (origin - self.target).normalized();
        let world_up = v3(0.0, 1.0, 0.0);
        let right = world_up.cross(back).normalized();
        let up = back.cross(right);

        let aspect = width as f64 / height as f64;
        let half_h = (degrees_to_radians(self.vfov) * 0.5).tan();
        let half_w = half_h * aspect;

        CameraBasis {
            origin,
            forward: -back,
            half_u: right * half_w,
            half_v: up * half_h,
            width: width as f64,
            height: height as f64,
        }
    }
}

impl CameraBasis {
    /// Rayo primario a traves del punto `(px, py)` del plano de imagen, medido en
    /// pixeles con el origen en la esquina superior izquierda. Los valores
    /// fraccionarios son los que producen el antialiasing por supermuestreo.
    #[inline]
    pub fn ray(&self, px: f64, py: f64) -> Ray {
        let sx = 2.0 * (px / self.width) - 1.0;
        let sy = 1.0 - 2.0 * (py / self.height);
        Ray::new(
            self.origin,
            self.forward + self.half_u * sx + self.half_v * sy,
        )
    }
}

/// Fotograma clave del recorrido automatico de demostracion.
#[derive(Debug, Clone, Copy)]
pub struct TourKey {
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub target: Vec3,
    /// Etiqueta de lo que ese tramo debe evidenciar, usada al nombrar fotogramas.
    pub label: &'static str,
}

/// Recorrido de demostracion: cada tramo enmarca una de las evidencias visuales
/// que el proyecto tiene que mostrar, en el orden en que aparecen en el guion.
pub fn tour_keys() -> Vec<TourKey> {
    let centro = v3(11.0, 9.5, 11.5);
    let estanque = v3(7.5, 6.0, 6.0);
    let vitral = v3(13.0, 15.0, 12.0);
    let altar = v3(13.0, 9.5, 19.0);
    let muro = v3(9.5, 11.0, 13.0);
    let poniente = v3(11.0, 13.0, 11.0);
    let isla = v3(11.0, 6.0, 11.0);
    // El azimut se deja correr siempre hacia valores menores y el ultimo
    // fotograma cierra exactamente una vuelta sobre el primero, de modo que el
    // recorrido encadena sin salto al repetirse.
    vec![
        TourKey {
            yaw: -147.0,
            pitch: 13.0,
            distance: 38.0,
            target: centro,
            label: "vista-general",
        },
        TourKey {
            yaw: -95.0,
            pitch: 17.0,
            distance: 38.0,
            target: centro,
            label: "rotacion",
        },
        TourKey {
            yaw: -60.0,
            pitch: 27.0,
            distance: 52.0,
            target: centro,
            label: "alejamiento",
        },
        TourKey {
            yaw: -172.0,
            pitch: 27.0,
            distance: 17.0,
            target: estanque,
            label: "agua-refraccion",
        },
        TourKey {
            yaw: -150.0,
            pitch: 10.0,
            distance: 20.0,
            target: vitral,
            label: "vitral",
        },
        TourKey {
            yaw: -196.0,
            pitch: 11.0,
            distance: 18.0,
            target: muro,
            label: "piedra-mapa-normal",
        },
        TourKey {
            yaw: -180.0,
            pitch: 6.0,
            distance: 27.0,
            target: altar,
            label: "emisores",
        },
        TourKey {
            yaw: -290.0,
            pitch: 5.0,
            distance: 60.0,
            target: poniente,
            label: "skybox",
        },
        // Tramo de enlace. Existe para repartir el giro: sin el, pasar del
        // contraluz al picado sobre el terreno obligaba a barrer ciento sesenta
        // grados de una vez y la camara se movia a tirones.
        TourKey {
            yaw: -370.0,
            pitch: 28.0,
            distance: 50.0,
            target: centro,
            label: "vuelta",
        },
        TourKey {
            yaw: -450.0,
            pitch: 50.0,
            distance: 46.0,
            target: isla,
            label: "terreno",
        },
        TourKey {
            yaw: -507.0,
            pitch: 13.0,
            distance: 38.0,
            target: centro,
            label: "cierre",
        },
    ]
}

/// Interpola el recorrido en `t` dentro de `[0, 1]`.
///
/// La interpolacion usa la quintica de Hermite en cada tramo, de modo que la
/// camara arranca y frena suavemente en cada fotograma clave en lugar de cambiar
/// de velocidad de golpe.
pub fn tour_camera(keys: &[TourKey], t: f64, vfov: f64) -> Camera {
    assert!(
        !keys.is_empty(),
        "el recorrido necesita al menos un fotograma"
    );
    if keys.len() == 1 {
        let k = keys[0];
        let mut c = Camera {
            target: k.target,
            yaw: k.yaw,
            pitch: k.pitch,
            distance: k.distance,
            vfov,
        };
        c.clamp();
        return c;
    }

    let t = t.clamp(0.0, 1.0);
    let segments = keys.len() - 1;
    let scaled = t * segments as f64;
    let i = (scaled.floor() as usize).min(segments - 1);
    let local = smootherstep(scaled - i as f64);
    let (a, b) = (keys[i], keys[i + 1]);

    let mut c = Camera {
        target: a.target.lerp(b.target, local),
        yaw: lerp(a.yaw, b.yaw, local),
        pitch: lerp(a.pitch, b.pitch, local),
        distance: lerp(a.distance, b.distance, local),
        vfov,
    };
    // El azimut del recorrido se deja correr por encima de 180 grados para poder
    // describir una vuelta completa, asi que aqui solo se limitan los ejes que
    // si tienen tope fisico.
    c.pitch = c.pitch.clamp(PITCH_MIN, PITCH_MAX);
    c.distance = c.distance.clamp(DISTANCE_MIN, DISTANCE_MAX);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_vista_inicial_esta_dentro_de_los_limites() {
        let mut c = Camera::initial();
        let antes = c;
        c.clamp();
        assert_eq!(antes, c, "la vista inicial no deberia necesitar recorte");
    }

    #[test]
    fn la_inclinacion_no_puede_cruzar_los_polos() {
        let mut c = Camera::initial();
        c.orbit(0.0, 500.0);
        assert!(c.pitch <= PITCH_MAX);
        c.orbit(0.0, -500.0);
        assert!(c.pitch >= PITCH_MIN);
    }

    #[test]
    fn la_base_de_la_vista_nunca_degenera_en_los_extremos() {
        for pitch in [PITCH_MIN, 45.0, PITCH_MAX] {
            for yaw in [-180.0, -90.0, 0.0, 90.0, 180.0] {
                let c = Camera {
                    yaw,
                    pitch,
                    ..Camera::initial()
                };
                let b = c.basis(320, 180);
                assert!(b.forward.is_finite() && b.half_u.is_finite() && b.half_v.is_finite());
                assert!(b.half_u.length() > 1e-6 && b.half_v.length() > 1e-6);
                // Los tres ejes de la vista siguen siendo mutuamente ortogonales.
                assert!(b.forward.dot(b.half_u).abs() < 1e-9);
                assert!(b.forward.dot(b.half_v).abs() < 1e-9);
                assert!(b.half_u.dot(b.half_v).abs() < 1e-9);
                // La imagen nunca queda cabeza abajo.
                assert!(b.half_v.y > 0.0);
            }
        }
    }

    #[test]
    fn el_zoom_respeta_sus_topes() {
        let mut c = Camera::initial();
        for _ in 0..100 {
            c.zoom(0.8);
        }
        assert!((c.distance - DISTANCE_MIN).abs() < 1e-9);
        for _ in 0..100 {
            c.zoom(1.25);
        }
        assert!((c.distance - DISTANCE_MAX).abs() < 1e-9);
    }

    #[test]
    fn la_camara_nunca_acaba_dentro_del_diorama() {
        // Volumen real del diorama: 24 x 30 x 24 celdas desde el origen.
        let caja = Aabb::new(v3(0.0, 0.0, 0.0), v3(24.0, 30.0, 24.0));
        for i in 0..72 {
            for pitch in [PITCH_MIN, 20.0, 45.0, PITCH_MAX] {
                for objetivo in [
                    v3(12.0, 9.0, 13.0),
                    v3(10.0, 6.5, 7.0),
                    v3(20.0, 14.0, 20.0),
                ] {
                    let mut c = Camera {
                        yaw: i as f64 * 5.0 - 180.0,
                        pitch,
                        distance: 1.0,
                        target: objetivo,
                        ..Camera::initial()
                    };
                    c.enforce_outside(caja, 1.0);
                    let p = c.position();
                    assert!(!caja.contains(p), "camara dentro de la escena en {p:?}");
                }
            }
        }
    }

    #[test]
    fn empujar_la_camara_fuera_no_reduce_una_distancia_ya_valida() {
        let caja = Aabb::new(v3(0.0, 0.0, 0.0), v3(24.0, 30.0, 24.0));
        let mut c = Camera::initial();
        c.distance = 55.0;
        c.enforce_outside(caja, 1.0);
        assert!((c.distance - 55.0).abs() < 1e-9);
    }

    #[test]
    fn el_rayo_central_apunta_al_objetivo() {
        let c = Camera::initial();
        let b = c.basis(800, 450);
        let r = b.ray(400.0, 225.0);
        let hacia = (c.target - r.origin).normalized();
        assert!((r.dir - hacia).length() < 1e-9);
    }

    #[test]
    fn los_rayos_de_las_esquinas_abren_el_campo_de_vision() {
        let c = Camera::initial();
        let b = c.basis(800, 450);
        let centro = b.ray(400.0, 225.0);
        let arriba = b.ray(400.0, 0.0);
        let angulo = centro.dir.dot(arriba.dir).clamp(-1.0, 1.0).acos();
        let esperado = degrees_to_radians(c.vfov) * 0.5;
        assert!((angulo - esperado).abs() < 1e-6);
    }

    #[test]
    fn el_recorrido_es_continuo_y_cubre_todas_las_evidencias() {
        let keys = tour_keys();
        assert!(keys.len() >= 9, "el guion pide nueve evidencias");
        let mut anterior = tour_camera(&keys, 0.0, 42.0);
        let pasos = 600;
        let mut saltos = Vec::with_capacity(pasos);
        for i in 1..=pasos {
            let c = tour_camera(&keys, i as f64 / pasos as f64, 42.0);
            saltos.push((c.position() - anterior.position()).length());
            assert!(c.pitch >= PITCH_MIN && c.pitch <= PITCH_MAX);
            assert!(c.distance >= DISTANCE_MIN && c.distance <= DISTANCE_MAX);
            anterior = c;
        }

        assert!(saltos.iter().any(|&s| s > 0.0), "la camara no se mueve");

        // Lo que hay que comprobar es continuidad, no velocidad: un tramo puede
        // barrer mas angulo que otro a proposito, y la quintica hace que la
        // camara casi se detenga en cada fotograma clave. Una discontinuidad de
        // verdad solo puede aparecer justo en una union, asi que es ahi donde se
        // mide, cruzandola con un paso minusculo.
        let segmentos = keys.len() - 1;
        for i in 1..segmentos {
            let t = i as f64 / segmentos as f64;
            let eps = 1e-6;
            let antes = tour_camera(&keys, t - eps, 42.0).position();
            let despues = tour_camera(&keys, t + eps, 42.0).position();
            let salto = (despues - antes).length();
            assert!(
                salto < 1e-3,
                "la union {i} no encadena: salto de {salto} unidades"
            );
        }
    }

    #[test]
    fn el_recorrido_cierra_el_bucle() {
        // El ultimo fotograma clave tiene que coincidir con el primero salvo por
        // una vuelta entera de azimut, para que el video se repita sin tiron.
        let keys = tour_keys();
        let primero = keys[0];
        let ultimo = keys[keys.len() - 1];
        assert!((ultimo.yaw - (primero.yaw - 360.0)).abs() < 1e-9);
        assert!((ultimo.pitch - primero.pitch).abs() < 1e-9);
        assert!((ultimo.distance - primero.distance).abs() < 1e-9);
        assert!((ultimo.target - primero.target).length() < 1e-9);

        let inicio = tour_camera(&keys, 0.0, 42.0);
        let fin = tour_camera(&keys, 1.0, 42.0);
        assert!((inicio.position() - fin.position()).length() < 1e-6);
    }

    #[test]
    fn el_recorrido_gira_y_tambien_acerca_y_aleja() {
        let keys = tour_keys();
        let muestras: Vec<Camera> = (0..=200)
            .map(|i| tour_camera(&keys, i as f64 / 200.0, 42.0))
            .collect();
        let yaw_min = muestras.iter().map(|c| c.yaw).fold(f64::INFINITY, f64::min);
        let yaw_max = muestras
            .iter()
            .map(|c| c.yaw)
            .fold(f64::NEG_INFINITY, f64::max);
        let d_min = muestras
            .iter()
            .map(|c| c.distance)
            .fold(f64::INFINITY, f64::min);
        let d_max = muestras
            .iter()
            .map(|c| c.distance)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            yaw_max - yaw_min > 180.0,
            "el recorrido debe rotar de verdad"
        );
        assert!(d_max / d_min > 1.8, "debe haber acercamiento y alejamiento");
    }
}
