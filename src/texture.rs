//! Texturas: muestreo por UV, decodificacion de sRGB y lectura de mapas normales.
//!
//! Las texturas del diorama son imagenes PPM guardadas en el repositorio, no
//! patrones evaluados en el momento del impacto. La diferencia importa: el
//! trazador hace una lectura de tabla por impacto en lugar de varias octavas de
//! ruido, y el resultado es identico en cualquier maquina.

use crate::image::Image;
use crate::math::{v3, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// Filtro de muestreo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// Vecino mas cercano: conserva el pixel duro de la estetica del diorama.
    Nearest,
    /// Bilineal: solo para el cielo, donde el degradado debe ser continuo.
    Bilinear,
}

/// Como se interpretan los valores almacenados en la imagen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Color percibido: hay que linealizarlo antes de iluminar.
    Srgb,
    /// Datos crudos, se usan tal cual. Es el caso de los mapas normales: pasar
    /// un vector por la curva de sRGB lo inclinaria de forma arbitraria.
    Linear,
}

/// Textura muestreable, con los valores ya convertidos a coma flotante.
///
/// La conversion se hace una sola vez al cargar. Un impacto solo paga el indexado
/// y, en el caso del cielo, cuatro lecturas y tres mezclas.
#[derive(Debug, Clone)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub filter: Filter,
    /// Valores por pixel, ya lineales si la codificacion era sRGB.
    pixels: Vec<Vec3>,
}

/// Convierte una componente de sRGB a luz lineal.
#[inline]
pub fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Convierte luz lineal a sRGB.
#[inline]
pub fn linear_to_srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

impl Texture {
    /// Construye una textura a partir de una imagen en memoria.
    pub fn from_image(img: &Image, encoding: Encoding, filter: Filter) -> Texture {
        let mut pixels = Vec::with_capacity(img.width * img.height);
        for y in 0..img.height {
            for x in 0..img.width {
                let [r, g, b] = img.get(x, y);
                let c = v3(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
                pixels.push(match encoding {
                    Encoding::Srgb => v3(
                        srgb_to_linear(c.x),
                        srgb_to_linear(c.y),
                        srgb_to_linear(c.z),
                    ),
                    Encoding::Linear => c,
                });
            }
        }
        Texture {
            width: img.width,
            height: img.height,
            filter,
            pixels,
        }
    }

    /// Carga un PPM del disco.
    pub fn load(
        path: impl AsRef<Path>,
        encoding: Encoding,
        filter: Filter,
    ) -> std::io::Result<Texture> {
        let img = Image::read_ppm(path)?;
        Ok(Texture::from_image(&img, encoding, filter))
    }

    /// Textura de un solo color, usada como reserva si falta un recurso.
    pub fn solid(color: Vec3) -> Texture {
        Texture {
            width: 1,
            height: 1,
            filter: Filter::Nearest,
            pixels: vec![color],
        }
    }

    #[inline]
    fn at(&self, x: usize, y: usize) -> Vec3 {
        self.pixels[y * self.width + x]
    }

    /// Envuelve una coordenada de textura en `[0, 1)` con repeticion.
    ///
    /// `rem_euclid` trata correctamente las UV negativas, que aparecen en cuanto
    /// se escala el mapeo para que una textura cubra varios bloques.
    #[inline]
    fn wrap(t: f64) -> f64 {
        let w = t.rem_euclid(1.0);
        if w.is_finite() {
            w
        } else {
            0.0
        }
    }

    /// Muestrea la textura en `(u, v)`, con `v = 0` en la fila superior.
    #[inline]
    pub fn sample(&self, u: f64, v: f64) -> Vec3 {
        match self.filter {
            Filter::Nearest => self.sample_nearest(u, v),
            Filter::Bilinear => self.sample_bilinear(u, v),
        }
    }

    #[inline]
    pub fn sample_nearest(&self, u: f64, v: f64) -> Vec3 {
        let x = (Texture::wrap(u) * self.width as f64) as usize;
        let y = (Texture::wrap(v) * self.height as f64) as usize;
        self.at(x.min(self.width - 1), y.min(self.height - 1))
    }

    /// Muestreo bilineal con bordes fijados.
    ///
    /// El borde se fija en lugar de repetirse porque el unico usuario es el
    /// cubemap: repetir mezclaria el ultimo texel de una cara con el primero,
    /// justo el artefacto de costura que hay que evitar.
    pub fn sample_bilinear(&self, u: f64, v: f64) -> Vec3 {
        let fx = u.clamp(0.0, 1.0) * self.width as f64 - 0.5;
        let fy = v.clamp(0.0, 1.0) * self.height as f64 - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let tx = fx - x0;
        let ty = fy - y0;
        let cx = |x: f64| (x.max(0.0) as usize).min(self.width - 1);
        let cy = |y: f64| (y.max(0.0) as usize).min(self.height - 1);
        let (x0, x1) = (cx(x0), cx(x0 + 1.0));
        let (y0, y1) = (cy(y0), cy(y0 + 1.0));
        let arriba = self.at(x0, y0).lerp(self.at(x1, y0), tx);
        let abajo = self.at(x0, y1).lerp(self.at(x1, y1), tx);
        arriba.lerp(abajo, ty)
    }

    /// Lee un mapa normal y devuelve el vector en espacio tangente.
    ///
    /// La imagen guarda `n * 0.5 + 0.5` por canal, la convencion habitual con `z`
    /// hacia fuera de la superficie; aqui se deshace ese cambio de escala. La
    /// componente `z` se fuerza a ser positiva para que un mapa mal codificado
    /// abolle la superficie pero nunca la vuelva del reves.
    #[inline]
    pub fn sample_normal(&self, u: f64, v: f64) -> Vec3 {
        let c = self.sample(u, v);
        v3(
            c.x * 2.0 - 1.0,
            c.y * 2.0 - 1.0,
            (c.z * 2.0 - 1.0).max(1e-3),
        )
        .normalized()
    }
}

/// Identificador de textura dentro de la coleccion.
pub type TextureId = usize;

/// Coleccion de texturas cargadas, indexadas por nombre y por identificador.
#[derive(Debug, Default)]
pub struct TextureSet {
    texturas: Vec<Texture>,
    por_nombre: HashMap<String, TextureId>,
}

impl TextureSet {
    pub fn new() -> TextureSet {
        TextureSet::default()
    }

    pub fn len(&self) -> usize {
        self.texturas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.texturas.is_empty()
    }

    /// Anade una textura ya construida y devuelve su identificador.
    pub fn push(&mut self, nombre: &str, textura: Texture) -> TextureId {
        let id = self.texturas.len();
        self.texturas.push(textura);
        self.por_nombre.insert(nombre.to_string(), id);
        id
    }

    /// Carga `<dir>/<nombre>.ppm`.
    ///
    /// Si el fichero no esta, se registra una textura lisa del color de reserva y
    /// se devuelve el aviso: preferimos una escena completa con un material plano
    /// a un fallo duro en medio de un render largo.
    pub fn load(
        &mut self,
        dir: &Path,
        nombre: &str,
        encoding: Encoding,
        filter: Filter,
        reserva: Vec3,
    ) -> (TextureId, Option<String>) {
        let ruta = dir.join(format!("{nombre}.ppm"));
        match Texture::load(&ruta, encoding, filter) {
            Ok(t) => (self.push(nombre, t), None),
            Err(e) => {
                let aviso = format!("no se pudo cargar {}: {e}", ruta.display());
                (self.push(nombre, Texture::solid(reserva)), Some(aviso))
            }
        }
    }

    #[inline]
    pub fn get(&self, id: TextureId) -> &Texture {
        &self.texturas[id]
    }

    pub fn id_of(&self, nombre: &str) -> Option<TextureId> {
        self.por_nombre.get(nombre).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tablero(w: usize, h: usize) -> Image {
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let c = if (x + y) % 2 == 0 { 255 } else { 0 };
                img.set(x, y, [c, c, c]);
            }
        }
        img
    }

    #[test]
    fn srgb_y_lineal_son_inversos() {
        for i in 0..=255 {
            let c = i as f64 / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(c)) - c).abs() < 1e-9);
        }
        assert!((srgb_to_linear(0.0)).abs() < 1e-12);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-12);
        // El gris medio percibido es bastante mas oscuro en luz lineal.
        assert!((srgb_to_linear(0.5) - 0.2140).abs() < 0.001);
    }

    #[test]
    fn el_muestreo_cercano_no_mezcla_texels() {
        let t = Texture::from_image(&tablero(4, 4), Encoding::Linear, Filter::Nearest);
        // Centro de cada texel: solo puede salir blanco o negro puro.
        for y in 0..4 {
            for x in 0..4 {
                let c = t.sample((x as f64 + 0.5) / 4.0, (y as f64 + 0.5) / 4.0);
                assert!(c.x == 0.0 || c.x == 1.0, "texel mezclado: {c:?}");
            }
        }
    }

    #[test]
    fn la_fila_superior_corresponde_a_v_cero() {
        let mut img = Image::new(1, 2);
        img.set(0, 0, [255, 0, 0]);
        img.set(0, 1, [0, 0, 255]);
        let t = Texture::from_image(&img, Encoding::Linear, Filter::Nearest);
        assert!(
            t.sample(0.5, 0.25).x > 0.9,
            "v=0 debe leer la fila de arriba"
        );
        assert!(
            t.sample(0.5, 0.75).z > 0.9,
            "v=1 debe leer la fila de abajo"
        );
    }

    #[test]
    fn las_uv_se_repiten_incluso_siendo_negativas() {
        let t = Texture::from_image(&tablero(4, 4), Encoding::Linear, Filter::Nearest);
        let base = t.sample(0.3, 0.7);
        assert_eq!(t.sample(3.3, 5.7), base);
        assert_eq!(t.sample(-0.7, -0.3), base);
    }

    #[test]
    fn el_muestreo_bilineal_promedia_y_fija_los_bordes() {
        let t = Texture::from_image(&tablero(2, 2), Encoding::Linear, Filter::Bilinear);
        // En el centro exacto de la imagen se promedian los cuatro texels.
        let centro = t.sample(0.5, 0.5);
        assert!((centro.x - 0.5).abs() < 1e-9, "centro = {centro:?}");
        // En las esquinas el borde fijado devuelve el texel puro, sin envolver.
        let esquina = t.sample(0.0, 0.0);
        assert!((esquina.x - 1.0).abs() < 1e-9);
        assert_eq!(t.sample(-5.0, -5.0), esquina);
    }

    #[test]
    fn un_mapa_normal_plano_devuelve_el_eje_z() {
        let mut img = Image::new(2, 2);
        for y in 0..2 {
            for x in 0..2 {
                img.set(x, y, [128, 128, 255]);
            }
        }
        let t = Texture::from_image(&img, Encoding::Linear, Filter::Nearest);
        let n = t.sample_normal(0.5, 0.5);
        assert!(
            n.z > 0.99,
            "una normal plana debe apuntar hacia fuera: {n:?}"
        );
        assert!(n.x.abs() < 0.01 && n.y.abs() < 0.01);
        assert!((n.length() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn el_mapa_normal_inclina_en_el_sentido_esperado() {
        let mut img = Image::new(2, 1);
        // Rojo alto: la normal se inclina hacia +u. Rojo bajo: hacia -u.
        img.set(0, 0, [230, 128, 200]);
        img.set(1, 0, [30, 128, 200]);
        let t = Texture::from_image(&img, Encoding::Linear, Filter::Nearest);
        let a = t.sample_normal(0.25, 0.5);
        let b = t.sample_normal(0.75, 0.5);
        assert!(a.x > 0.3, "deberia inclinarse hacia +u: {a:?}");
        assert!(b.x < -0.3, "deberia inclinarse hacia -u: {b:?}");
        assert!(a.z > 0.0 && b.z > 0.0, "nunca debe mirar hacia dentro");
    }

    #[test]
    fn un_mapa_normal_invertido_no_atraviesa_la_superficie() {
        let mut img = Image::new(1, 1);
        // Canal azul a cero: codificacion erronea con z negativa.
        img.set(0, 0, [128, 128, 0]);
        let t = Texture::from_image(&img, Encoding::Linear, Filter::Nearest);
        assert!(t.sample_normal(0.5, 0.5).z > 0.0);
    }

    #[test]
    fn la_coleccion_indexa_por_nombre_y_avisa_si_falta_un_recurso() {
        let mut set = TextureSet::new();
        let id = set.push("piedra", Texture::solid(v3(0.5, 0.5, 0.5)));
        assert_eq!(set.id_of("piedra"), Some(id));
        assert_eq!(set.id_of("inexistente"), None);
        assert_eq!(set.len(), 1);

        let dir = std::env::temp_dir().join("abadia_sin_texturas");
        let (id2, aviso) = set.load(&dir, "ausente", Encoding::Srgb, Filter::Nearest, Vec3::ONE);
        assert!(aviso.is_some(), "debe avisar de la textura ausente");
        assert_eq!(set.get(id2).sample(0.0, 0.0), Vec3::ONE);
    }

    #[test]
    fn la_codificacion_srgb_se_aplica_al_cargar() {
        let mut img = Image::new(1, 1);
        img.set(0, 0, [128, 128, 128]);
        let srgb = Texture::from_image(&img, Encoding::Srgb, Filter::Nearest);
        let lineal = Texture::from_image(&img, Encoding::Linear, Filter::Nearest);
        assert!(srgb.sample(0.0, 0.0).x < lineal.sample(0.0, 0.0).x);
        assert!((srgb.sample(0.0, 0.0).x - srgb_to_linear(128.0 / 255.0)).abs() < 1e-12);
    }
}
