//! Escritura de GIF animado, tambien sin dependencias.
//!
//! El encargo pide un video demostrativo en el README. Un video hay que grabarlo
//! y codificarlo con herramientas ajenas al proyecto, pero el recorrido si se
//! puede entregar como animacion que GitHub muestra directamente. Eso exige dos
//! piezas que no trae la biblioteca estandar: reducir la escena a 256 colores y
//! comprimir con LZW.
//!
//! El formato no admite mas de 256 colores por imagen, asi que la paleta se
//! calcula a partir de los propios fotogramas con corte por la mediana. Es la
//! diferencia entre una animacion fiel y una con el cielo a bandas: una paleta
//! fija tendria que repartir sus entradas por todo el espacio de color, mientras
//! que el anochecer de esta escena vive en una franja estrecha de violetas,
//! grises y ambares.

use crate::image::Image;
use std::io::Write;

/// Color de la paleta.
type Rgb = [u8; 3];

/// Caja del algoritmo de corte por la mediana.
struct Caja {
    colores: Vec<Rgb>,
}

impl Caja {
    /// Eje con mayor rango. Es por donde conviene partir: reduce mas el error.
    fn eje_mas_largo(&self) -> usize {
        let mut min = [255u8; 3];
        let mut max = [0u8; 3];
        for c in &self.colores {
            for k in 0..3 {
                min[k] = min[k].min(c[k]);
                max[k] = max[k].max(c[k]);
            }
        }
        let rangos = [
            max[0] as i32 - min[0] as i32,
            max[1] as i32 - min[1] as i32,
            max[2] as i32 - min[2] as i32,
        ];
        let mut eje = 0;
        for k in 1..3 {
            if rangos[k] > rangos[eje] {
                eje = k;
            }
        }
        eje
    }

    fn rango(&self) -> i32 {
        let eje = self.eje_mas_largo();
        let mut min = 255i32;
        let mut max = 0i32;
        for c in &self.colores {
            min = min.min(c[eje] as i32);
            max = max.max(c[eje] as i32);
        }
        max - min
    }

    /// Color representativo: la media de la caja.
    fn media(&self) -> Rgb {
        let mut suma = [0u64; 3];
        for c in &self.colores {
            for k in 0..3 {
                suma[k] += c[k] as u64;
            }
        }
        let n = self.colores.len().max(1) as u64;
        [
            (suma[0] / n) as u8,
            (suma[1] / n) as u8,
            (suma[2] / n) as u8,
        ]
    }
}

/// Calcula una paleta de como mucho `maximo` colores por corte de la mediana.
pub fn quantize(muestras: &[Rgb], maximo: usize) -> Vec<Rgb> {
    if muestras.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut cajas = vec![Caja {
        colores: muestras.to_vec(),
    }];

    while cajas.len() < maximo {
        // Se parte siempre la caja con mayor rango: es la que mas error aporta.
        let mut mejor = None;
        let mut mejor_rango = 1;
        for (i, c) in cajas.iter().enumerate() {
            if c.colores.len() < 2 {
                continue;
            }
            let r = c.rango();
            if r > mejor_rango {
                mejor_rango = r;
                mejor = Some(i);
            }
        }
        let Some(i) = mejor else { break };

        let mut caja = cajas.swap_remove(i);
        let eje = caja.eje_mas_largo();
        caja.colores.sort_by_key(|c| c[eje]);
        let mitad = caja.colores.len() / 2;
        let derecha = caja.colores.split_off(mitad);
        cajas.push(Caja {
            colores: caja.colores,
        });
        cajas.push(Caja { colores: derecha });
    }

    cajas.into_iter().map(|c| c.media()).collect()
}

/// Tabla de busqueda del color mas cercano, indexada por los cinco bits altos de
/// cada canal.
///
/// Sin ella, cada pixel de cada fotograma costaria una busqueda lineal sobre las
/// 256 entradas de la paleta. La tabla se calcula una vez, tiene 32768 entradas y
/// convierte esa busqueda en un indexado.
pub struct PaletteLookup {
    palette: Vec<Rgb>,
    tabla: Vec<u8>,
}

impl PaletteLookup {
    pub fn new(palette: Vec<Rgb>) -> PaletteLookup {
        let mut tabla = vec![0u8; 32768];
        for (i, entrada) in tabla.iter_mut().enumerate() {
            let r = ((i >> 10) & 31) as i32 * 8 + 4;
            let g = ((i >> 5) & 31) as i32 * 8 + 4;
            let b = (i & 31) as i32 * 8 + 4;
            let mut mejor = 0usize;
            let mut mejor_d = i64::MAX;
            for (k, c) in palette.iter().enumerate() {
                let dr = r - c[0] as i32;
                let dg = g - c[1] as i32;
                let db = b - c[2] as i32;
                // Distancia ponderada por sensibilidad del ojo.
                let d = 2 * (dr * dr) as i64 + 4 * (dg * dg) as i64 + 3 * (db * db) as i64;
                if d < mejor_d {
                    mejor_d = d;
                    mejor = k;
                }
            }
            *entrada = mejor as u8;
        }
        PaletteLookup { palette, tabla }
    }

    #[inline]
    pub fn index_of(&self, rgb: Rgb) -> u8 {
        let i =
            ((rgb[0] as usize >> 3) << 10) | ((rgb[1] as usize >> 3) << 5) | (rgb[2] as usize >> 3);
        self.tabla[i]
    }

    pub fn palette(&self) -> &[Rgb] {
        &self.palette
    }
}

/// Matriz de Bayer de 4x4, en el rango `[-8, 7]`.
///
/// Se suma antes de cuantizar. El tramado ordenado cuesta una suma por canal y
/// es lo que evita que el degradado del cielo salga a bandas, que es donde mas se
/// nota la reduccion a 256 colores.
const BAYER: [[i32; 4]; 4] = [
    [-8, 0, -6, 2],
    [4, -4, 6, -2],
    [-5, 3, -7, 1],
    [7, -1, 5, -3],
];

/// Convierte una imagen a indices de paleta, con tramado ordenado.
pub fn index_image(img: &Image, lut: &PaletteLookup, dither: i32) -> Vec<u8> {
    let mut salida = vec![0u8; img.width * img.height];
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.get(x, y);
            let d = BAYER[y & 3][x & 3] * dither / 8;
            let ajustado = [
                (p[0] as i32 + d).clamp(0, 255) as u8,
                (p[1] as i32 + d).clamp(0, 255) as u8,
                (p[2] as i32 + d).clamp(0, 255) as u8,
            ];
            salida[y * img.width + x] = lut.index_of(ajustado);
        }
    }
    salida
}

/// Escritor de bits para LZW: los codigos entran por el extremo menos
/// significativo, igual que en deflate, pero con anchura variable.
struct BitWriter {
    datos: Vec<u8>,
    acumulador: u32,
    bits: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter {
            datos: Vec::new(),
            acumulador: 0,
            bits: 0,
        }
    }

    fn write(&mut self, codigo: u32, ancho: u32) {
        self.acumulador |= codigo << self.bits;
        self.bits += ancho;
        while self.bits >= 8 {
            self.datos.push((self.acumulador & 0xFF) as u8);
            self.acumulador >>= 8;
            self.bits -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.datos.push((self.acumulador & 0xFF) as u8);
        }
        self.datos
    }
}

/// Compresion LZW de GIF sobre una imagen ya indexada.
pub fn lzw_encode(indices: &[u8], min_code_size: u32) -> Vec<u8> {
    let clear = 1u32 << min_code_size;
    let fin = clear + 1;
    let mut ancho = min_code_size + 1;
    let mut siguiente = fin + 1;

    // Diccionario plano: `dic[prefijo * 256 + byte]` guarda el codigo, o cero si
    // la pareja no esta. Se reserva el cero como "vacio" porque ningun codigo
    // util puede valer menos que `clear + 2`.
    let mut dic = vec![0u16; 4096 * 256];
    let mut bw = BitWriter::new();
    bw.write(clear, ancho);

    if indices.is_empty() {
        bw.write(fin, ancho);
        return bw.finish();
    }

    let mut prefijo = indices[0] as u32;
    for &k in &indices[1..] {
        let clave = prefijo as usize * 256 + k as usize;
        let existente = dic[clave];
        if existente != 0 {
            prefijo = existente as u32;
            continue;
        }
        bw.write(prefijo, ancho);
        if siguiente < 4096 {
            dic[clave] = siguiente as u16;
            siguiente += 1;
            if siguiente > (1 << ancho) && ancho < 12 {
                ancho += 1;
            }
        } else {
            // Diccionario lleno: se reinicia, que es lo que espera el lector.
            bw.write(clear, ancho);
            dic.iter_mut().for_each(|v| *v = 0);
            ancho = min_code_size + 1;
            siguiente = fin + 1;
        }
        prefijo = k as u32;
    }
    bw.write(prefijo, ancho);
    bw.write(fin, ancho);
    bw.finish()
}

/// Parte un flujo en los subbloques de como mucho 255 bytes que exige GIF.
fn subbloques(datos: &[u8], salida: &mut Vec<u8>) {
    for trozo in datos.chunks(255) {
        salida.push(trozo.len() as u8);
        salida.extend_from_slice(trozo);
    }
    salida.push(0);
}

/// Un GIF animado en construccion.
pub struct GifWriter {
    ancho: u16,
    alto: u16,
    lut: PaletteLookup,
    bytes: Vec<u8>,
    /// Centesimas de segundo entre fotogramas.
    delay: u16,
    fotogramas: usize,
}

impl GifWriter {
    /// Prepara la cabecera con la paleta global.
    pub fn new(ancho: usize, alto: usize, lut: PaletteLookup, delay_cs: u16) -> GifWriter {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GIF89a");
        bytes.extend_from_slice(&(ancho as u16).to_le_bytes());
        bytes.extend_from_slice(&(alto as u16).to_le_bytes());
        // Paleta global de 256 entradas, ocho bits de resolucion.
        bytes.push(0b1111_0111);
        bytes.push(0); // color de fondo
        bytes.push(0); // relacion de aspecto no especificada

        let paleta = lut.palette();
        for i in 0..256 {
            let c = paleta.get(i).copied().unwrap_or([0, 0, 0]);
            bytes.extend_from_slice(&c);
        }

        // Extension de aplicacion Netscape: repeticion indefinida.
        bytes.extend_from_slice(&[0x21, 0xFF, 0x0B]);
        bytes.extend_from_slice(b"NETSCAPE2.0");
        bytes.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);

        GifWriter {
            ancho: ancho as u16,
            alto: alto as u16,
            lut,
            bytes,
            delay: delay_cs,
            fotogramas: 0,
        }
    }

    /// Anade un fotograma.
    pub fn add_frame(&mut self, img: &Image, dither: i32) -> Result<(), String> {
        if img.width != self.ancho as usize || img.height != self.alto as usize {
            return Err(format!(
                "el fotograma mide {}x{} y la animacion {}x{}",
                img.width, img.height, self.ancho, self.alto
            ));
        }
        let indices = index_image(img, &self.lut, dither);

        // Control grafico: retardo y sin transparencia.
        self.bytes.extend_from_slice(&[0x21, 0xF9, 0x04, 0x04]);
        self.bytes.extend_from_slice(&self.delay.to_le_bytes());
        self.bytes.extend_from_slice(&[0x00, 0x00]);

        // Descriptor de imagen a pantalla completa, sin paleta local.
        self.bytes.push(0x2C);
        self.bytes.extend_from_slice(&0u16.to_le_bytes());
        self.bytes.extend_from_slice(&0u16.to_le_bytes());
        self.bytes.extend_from_slice(&self.ancho.to_le_bytes());
        self.bytes.extend_from_slice(&self.alto.to_le_bytes());
        self.bytes.push(0x00);

        self.bytes.push(8); // tamano minimo de codigo
        let comprimido = lzw_encode(&indices, 8);
        subbloques(&comprimido, &mut self.bytes);
        self.fotogramas += 1;
        Ok(())
    }

    pub fn frame_count(&self) -> usize {
        self.fotogramas
    }

    /// Cierra el fichero y lo escribe en disco.
    pub fn finish(mut self, path: impl AsRef<std::path::Path>) -> std::io::Result<usize> {
        self.bytes.push(0x3B);
        if let Some(parent) = path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let n = self.bytes.len();
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        f.write_all(&self.bytes)?;
        f.flush()?;
        Ok(n)
    }
}

/// Recoge muestras de color de una imagen, tomando uno de cada `paso` pixeles.
pub fn sample_colors(img: &Image, paso: usize, destino: &mut Vec<Rgb>) {
    let paso = paso.max(1);
    let total = img.width * img.height;
    let mut i = 0;
    while i < total {
        let x = i % img.width;
        let y = i / img.width;
        destino.push(img.get(x, y));
        i += paso;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Descompresor LZW de GIF, solo para las pruebas.
    ///
    /// Cierra el ciclo sobre el compresor igual que hace el inflater del modulo
    /// de imagen: la validez del flujo no depende de ninguna herramienta externa.
    fn lzw_decode(datos: &[u8], min_code_size: u32) -> Vec<u8> {
        let clear = 1u32 << min_code_size;
        let fin = clear + 1;
        let mut ancho = min_code_size + 1;
        let mut dic: Vec<Vec<u8>> = (0..clear).map(|i| vec![i as u8]).collect();
        dic.push(Vec::new()); // clear
        dic.push(Vec::new()); // fin

        let mut salida = Vec::new();
        let mut bit = 0usize;
        let leer = |bit: &mut usize, ancho: u32| -> Option<u32> {
            let mut v = 0u32;
            for i in 0..ancho {
                let byte = *bit / 8;
                if byte >= datos.len() {
                    return None;
                }
                let b = (datos[byte] >> (*bit % 8)) & 1;
                v |= (b as u32) << i;
                *bit += 1;
            }
            Some(v)
        };

        let mut anterior: Option<u32> = None;
        while let Some(codigo) = leer(&mut bit, ancho) {
            if codigo == clear {
                dic.truncate((clear + 2) as usize);
                ancho = min_code_size + 1;
                anterior = None;
                continue;
            }
            if codigo == fin {
                break;
            }
            let entrada = if (codigo as usize) < dic.len() {
                dic[codigo as usize].clone()
            } else {
                // Caso KwKwK: el codigo se acaba de crear en esta iteracion.
                let prev = dic[anterior.expect("codigo nuevo sin anterior") as usize].clone();
                let mut e = prev.clone();
                e.push(prev[0]);
                e
            };
            salida.extend_from_slice(&entrada);
            if let Some(p) = anterior {
                let mut nueva = dic[p as usize].clone();
                nueva.push(entrada[0]);
                dic.push(nueva);
                if dic.len() as u32 >= (1 << ancho) && ancho < 12 {
                    ancho += 1;
                }
            }
            anterior = Some(codigo);
        }
        salida
    }

    fn imagen(w: usize, h: usize) -> Image {
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.set(
                    x,
                    y,
                    [
                        (x * 255 / w.max(1)) as u8,
                        (y * 255 / h.max(1)) as u8,
                        ((x + y) % 64 * 4) as u8,
                    ],
                );
            }
        }
        img
    }

    #[test]
    fn lzw_reconstruye_exactamente_los_indices() {
        let casos: Vec<Vec<u8>> = vec![
            vec![0],
            vec![7; 500],
            (0..=255u8).collect(),
            (0..5000).map(|i| (i * 13 % 251) as u8).collect(),
            (0..70_000).map(|i| ((i / 97) % 256) as u8).collect(),
        ];
        for datos in casos {
            let comprimido = lzw_encode(&datos, 8);
            assert_eq!(lzw_decode(&comprimido, 8), datos, "len={}", datos.len());
        }
    }

    #[test]
    fn lzw_comprime_las_zonas_lisas() {
        let datos = vec![42u8; 100_000];
        let ratio = datos.len() as f64 / lzw_encode(&datos, 8).len() as f64;
        assert!(ratio > 20.0, "ratio insuficiente: {ratio}");
    }

    #[test]
    fn la_paleta_no_supera_el_maximo_y_cubre_los_colores() {
        let muestras: Vec<Rgb> = (0..4000)
            .map(|i| {
                let a = i as f64 * 0.013;
                [
                    (a.sin().abs() * 255.0) as u8,
                    (a.cos().abs() * 255.0) as u8,
                    ((a * 2.0).sin().abs() * 255.0) as u8,
                ]
            })
            .collect();
        let p = quantize(&muestras, 256);
        assert!(!p.is_empty() && p.len() <= 256, "paleta de {}", p.len());

        // El error medio de cuantizacion tiene que ser pequeno.
        let lut = PaletteLookup::new(p);
        let mut error = 0.0;
        for m in &muestras {
            let c = lut.palette()[lut.index_of(*m) as usize];
            error += ((m[0] as f64 - c[0] as f64).powi(2)
                + (m[1] as f64 - c[1] as f64).powi(2)
                + (m[2] as f64 - c[2] as f64).powi(2))
            .sqrt();
        }
        error /= muestras.len() as f64;
        assert!(error < 22.0, "error de cuantizacion alto: {error}");
    }

    #[test]
    fn una_imagen_de_pocos_colores_se_reproduce_sin_perdida() {
        // Con menos de 256 colores distintos, la paleta debe poder representarlos
        // todos y el indexado ser exacto.
        let mut img = Image::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                img.set(x, y, [(x * 16) as u8, (y * 16) as u8, 0]);
            }
        }
        let mut muestras = Vec::new();
        sample_colors(&img, 1, &mut muestras);
        let lut = PaletteLookup::new(quantize(&muestras, 256));
        let indices = index_image(&img, &lut, 0);
        for y in 0..16 {
            for x in 0..16 {
                let original = img.get(x, y);
                let recuperado = lut.palette()[indices[y * 16 + x] as usize];
                for k in 0..3 {
                    let d = (original[k] as i32 - recuperado[k] as i32).abs();
                    assert!(d <= 8, "desvio de {d} en el canal {k}");
                }
            }
        }
    }

    #[test]
    fn la_estructura_del_gif_es_valida() {
        let img = imagen(32, 24);
        let mut muestras = Vec::new();
        sample_colors(&img, 3, &mut muestras);
        let lut = PaletteLookup::new(quantize(&muestras, 256));
        let mut gif = GifWriter::new(32, 24, lut, 8);
        gif.add_frame(&img, 6).unwrap();
        gif.add_frame(&img, 6).unwrap();
        assert_eq!(gif.frame_count(), 2);

        let ruta = std::env::temp_dir().join("abadia_test.gif");
        let n = gif.finish(&ruta).unwrap();
        let bytes = std::fs::read(&ruta).unwrap();
        assert_eq!(bytes.len(), n);

        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 32);
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 24);
        assert_eq!(bytes[10] & 0x80, 0x80, "debe declarar paleta global");
        assert_eq!(bytes[10] & 0x07, 7, "paleta de 256 entradas");
        assert_eq!(*bytes.last().unwrap(), 0x3B, "falta el cierre");
        // Cabecera 13 + paleta 768 + extension Netscape 19.
        assert!(bytes.len() > 13 + 768 + 19);
        assert_eq!(&bytes[13 + 768..13 + 768 + 3], &[0x21, 0xFF, 0x0B]);
        // Dos descriptores de imagen.
        assert_eq!(bytes.iter().filter(|&&b| b == 0x2C).count() >= 2, true);
        std::fs::remove_file(&ruta).ok();
    }

    #[test]
    fn el_fotograma_de_otro_tamano_se_rechaza() {
        let lut = PaletteLookup::new(vec![[0, 0, 0]]);
        let mut gif = GifWriter::new(8, 8, lut, 5);
        assert!(gif.add_frame(&Image::new(9, 8), 0).is_err());
        assert!(gif.add_frame(&Image::new(8, 8), 0).is_ok());
    }

    #[test]
    fn el_tramado_rompe_las_bandas_de_un_degradado() {
        // Degradado suave con una paleta pobre: sin tramado, columnas enteras
        // comparten indice; con tramado, alternan.
        let mut img = Image::new(64, 8);
        for y in 0..8 {
            for x in 0..64 {
                let v = (x * 255 / 63) as u8;
                img.set(x, y, [v, v, v]);
            }
        }
        let lut = PaletteLookup::new(quantize(
            &(0..8).map(|i| [(i * 32) as u8; 3]).collect::<Vec<_>>(),
            8,
        ));
        let sin = index_image(&img, &lut, 0);
        let con = index_image(&img, &lut, 16);

        let variacion = |ind: &[u8]| {
            let mut cambios = 0;
            for x in 0..64 {
                let col: Vec<u8> = (0..8).map(|y| ind[y * 64 + x]).collect();
                if col.iter().any(|v| *v != col[0]) {
                    cambios += 1;
                }
            }
            cambios
        };
        assert_eq!(variacion(&sin), 0, "sin tramado las columnas son uniformes");
        assert!(variacion(&con) > 10, "el tramado no esta actuando");
    }
}
