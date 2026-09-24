//! Entrada y salida de imagen sin dependencias externas.
//!
//! Se implementan dos formatos. PPM binario (`P6`) es el que usan los recursos
//! del repositorio: es trivial de escribir y de leer, y basta para las texturas y
//! los mapas normales. PNG es el formato de salida para capturas, porque es el
//! unico de los dos que un navegador o el README pueden mostrar directamente; su
//! compresion `deflate` con Huffman fijo y busqueda LZ77 tambien esta escrita
//! aqui, junto con CRC-32 y Adler-32.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

/// Imagen en memoria, ocho bits por canal y tres canales entrelazados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// `width * height * 3` bytes en orden RGB.
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(width: usize, height: usize) -> Image {
        Image {
            width,
            height,
            data: vec![0; width * height * 3],
        }
    }

    #[inline]
    pub fn index(&self, x: usize, y: usize) -> usize {
        (y * self.width + x) * 3
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        let i = self.index(x, y);
        self.data[i] = rgb[0];
        self.data[i + 1] = rgb[1];
        self.data[i + 2] = rgb[2];
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> [u8; 3] {
        let i = self.index(x, y);
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    /// Guarda eligiendo el formato por la extension del nombre.
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        match ext.as_str() {
            "ppm" => self.write_ppm(path),
            _ => self.write_png(path),
        }
    }

    /// Escribe PPM binario `P6`.
    pub fn write_ppm(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let mut out = BufWriter::new(File::create(path)?);
        write!(out, "P6\n{} {}\n255\n", self.width, self.height)?;
        out.write_all(&self.data)?;
        out.flush()
    }

    /// Lee PPM `P6` (binario) o `P3` (texto), admitiendo comentarios.
    pub fn read_ppm(path: impl AsRef<Path>) -> std::io::Result<Image> {
        let path = path.as_ref();
        let mut bytes = Vec::new();
        File::open(path)?.read_to_end(&mut bytes)?;
        Image::from_ppm_bytes(&bytes).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: {e}", path.display()),
            )
        })
    }

    /// Analiza el contenido de un PPM ya cargado en memoria.
    pub fn from_ppm_bytes(bytes: &[u8]) -> Result<Image, String> {
        let mut cursor = 0usize;

        // Los campos de cabecera se separan por espacios en blanco y pueden
        // llevar comentarios `#` hasta el final de la linea.
        fn token(bytes: &[u8], cursor: &mut usize) -> Result<String, String> {
            loop {
                while *cursor < bytes.len() && bytes[*cursor].is_ascii_whitespace() {
                    *cursor += 1;
                }
                if *cursor < bytes.len() && bytes[*cursor] == b'#' {
                    while *cursor < bytes.len() && bytes[*cursor] != b'\n' {
                        *cursor += 1;
                    }
                } else {
                    break;
                }
            }
            let inicio = *cursor;
            while *cursor < bytes.len() && !bytes[*cursor].is_ascii_whitespace() {
                *cursor += 1;
            }
            if inicio == *cursor {
                return Err("cabecera PPM incompleta".to_string());
            }
            String::from_utf8(bytes[inicio..*cursor].to_vec())
                .map_err(|_| "cabecera PPM no es ASCII".to_string())
        }

        let magic = token(bytes, &mut cursor)?;
        let width: usize = token(bytes, &mut cursor)?
            .parse()
            .map_err(|_| "anchura invalida".to_string())?;
        let height: usize = token(bytes, &mut cursor)?
            .parse()
            .map_err(|_| "altura invalida".to_string())?;
        let maxval: usize = token(bytes, &mut cursor)?
            .parse()
            .map_err(|_| "valor maximo invalido".to_string())?;
        if maxval == 0 || maxval > 255 {
            return Err(format!("solo se admiten 8 bits por canal, no {maxval}"));
        }
        if width == 0 || height == 0 {
            return Err("imagen vacia".to_string());
        }

        let esperado = width * height * 3;
        let mut data = Vec::with_capacity(esperado);

        match magic.as_str() {
            "P6" => {
                // Exactamente un byte de separacion tras el valor maximo.
                cursor += 1;
                if bytes.len() < cursor + esperado {
                    return Err(format!(
                        "faltan datos: hay {} bytes y se esperaban {esperado}",
                        bytes.len().saturating_sub(cursor)
                    ));
                }
                data.extend_from_slice(&bytes[cursor..cursor + esperado]);
            }
            "P3" => {
                for _ in 0..esperado {
                    let v: usize = token(bytes, &mut cursor)?
                        .parse()
                        .map_err(|_| "muestra no numerica".to_string())?;
                    data.push(v.min(255) as u8);
                }
            }
            otro => return Err(format!("formato PPM no admitido: {otro}")),
        }

        // Reescala si el maximo declarado no era 255.
        if maxval != 255 {
            for b in data.iter_mut() {
                *b = ((*b as usize * 255) / maxval).min(255) as u8;
            }
        }

        Ok(Image {
            width,
            height,
            data,
        })
    }

    /// Escribe un PNG de color verdadero, ocho bits por canal.
    pub fn write_png(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&self.to_png_bytes())?;
        out.flush()
    }

    /// Serializa la imagen como PNG en memoria.
    pub fn to_png_bytes(&self) -> Vec<u8> {
        let mut png = Vec::with_capacity(self.data.len() / 2 + 1024);
        png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&(self.width as u32).to_be_bytes());
        ihdr.extend_from_slice(&(self.height as u32).to_be_bytes());
        ihdr.push(8); // profundidad de bits
        ihdr.push(2); // color verdadero sin alfa
        ihdr.push(0); // compresion deflate
        ihdr.push(0); // filtrado adaptativo estandar
        ihdr.push(0); // sin entrelazado
        write_chunk(&mut png, b"IHDR", &ihdr);

        let filtradas = self.filter_scanlines();
        write_chunk(&mut png, b"IDAT", &zlib_compress(&filtradas));
        write_chunk(&mut png, b"IEND", &[]);
        png
    }

    /// Aplica el filtrado por fila que exige PNG antes de comprimir.
    ///
    /// Se prueban los cinco filtros y se conserva el que minimiza la suma de
    /// magnitudes de los residuos, que es la heuristica habitual: sobre las
    /// imagenes del diorama reduce el flujo a comprimir a menos de la mitad.
    fn filter_scanlines(&self) -> Vec<u8> {
        const BPP: usize = 3;
        let stride = self.width * BPP;
        let mut salida = Vec::with_capacity((stride + 1) * self.height);
        let mut anterior = vec![0u8; stride];
        let mut candidatas = [
            vec![0u8; stride],
            vec![0u8; stride],
            vec![0u8; stride],
            vec![0u8; stride],
            vec![0u8; stride],
        ];

        for y in 0..self.height {
            let fila = &self.data[y * stride..(y + 1) * stride];
            for i in 0..stride {
                let a = if i >= BPP { fila[i - BPP] } else { 0 }; // izquierda
                let b = anterior[i]; // arriba
                let c = if i >= BPP { anterior[i - BPP] } else { 0 }; // diagonal
                let x = fila[i];
                candidatas[0][i] = x;
                candidatas[1][i] = x.wrapping_sub(a);
                candidatas[2][i] = x.wrapping_sub(b);
                candidatas[3][i] = x.wrapping_sub(((a as u16 + b as u16) / 2) as u8);
                candidatas[4][i] = x.wrapping_sub(paeth(a, b, c));
            }

            let mut mejor = 0usize;
            let mut mejor_coste = u64::MAX;
            for (f, cand) in candidatas.iter().enumerate() {
                // Los residuos se interpretan con signo: 200 es un residuo de -56.
                let coste: u64 = cand.iter().map(|&v| (v as i8).unsigned_abs() as u64).sum();
                if coste < mejor_coste {
                    mejor_coste = coste;
                    mejor = f;
                }
            }

            salida.push(mejor as u8);
            salida.extend_from_slice(&candidatas[mejor]);
            anterior.copy_from_slice(fila);
        }
        salida
    }
}

/// Predictor Paeth de PNG.
#[inline]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn write_chunk(out: &mut Vec<u8>, tipo: &[u8; 4], datos: &[u8]) {
    out.extend_from_slice(&(datos.len() as u32).to_be_bytes());
    let inicio = out.len();
    out.extend_from_slice(tipo);
    out.extend_from_slice(datos);
    let crc = crc32(&out[inicio..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// CRC-32 con el polinomio de PNG, calculado con tabla generada al vuelo.
pub fn crc32(datos: &[u8]) -> u32 {
    static TABLA: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let tabla = TABLA.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, entrada) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *entrada = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for &b in datos {
        c = tabla[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// Suma de comprobacion Adler-32 que cierra el flujo zlib.
pub fn adler32(datos: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    // 5552 es el mayor numero de iteraciones que no puede desbordar u32.
    for trozo in datos.chunks(5552) {
        for &byte in trozo {
            a += byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// Envuelve el flujo deflate en la cabecera zlib que espera PNG.
pub fn zlib_compress(datos: &[u8]) -> Vec<u8> {
    let mut salida = Vec::with_capacity(datos.len() / 2 + 64);
    // CMF = deflate con ventana de 32 KiB, FLG elegido para que (CMF<<8|FLG) % 31 == 0.
    salida.push(0x78);
    salida.push(0x01);
    salida.extend_from_slice(&deflate_fixed(datos));
    salida.extend_from_slice(&adler32(datos).to_be_bytes());
    salida
}

/// Escritor de bits en el orden que define deflate: los bits sueltos entran por
/// el extremo menos significativo del byte, mientras que los codigos Huffman se
/// escriben empezando por su bit mas significativo.
struct BitWriter {
    out: Vec<u8>,
    acumulador: u32,
    bits: u32,
}

impl BitWriter {
    fn new(capacidad: usize) -> BitWriter {
        BitWriter {
            out: Vec::with_capacity(capacidad),
            acumulador: 0,
            bits: 0,
        }
    }

    #[inline]
    fn write_bits(&mut self, valor: u32, cantidad: u32) {
        self.acumulador |= (valor & ((1u32 << cantidad) - 1)) << self.bits;
        self.bits += cantidad;
        while self.bits >= 8 {
            self.out.push((self.acumulador & 0xFF) as u8);
            self.acumulador >>= 8;
            self.bits -= 8;
        }
    }

    /// Escribe un codigo Huffman, invirtiendo el orden de sus bits.
    #[inline]
    fn write_code(&mut self, codigo: u32, longitud: u32) {
        let mut invertido = 0u32;
        for i in 0..longitud {
            invertido |= ((codigo >> (longitud - 1 - i)) & 1) << i;
        }
        self.write_bits(invertido, longitud);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.out.push((self.acumulador & 0xFF) as u8);
        }
        self.out
    }
}

/// Bases de longitud de los codigos 257..=285.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
/// Bits extra de longitud de los codigos 257..=285.
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Bases de distancia de los codigos 0..=29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// Bits extra de distancia de los codigos 0..=29.
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const WINDOW: usize = 32_768;
const HASH_BITS: u32 = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
/// Tope de la cadena de candidatos por posicion. Limitar la busqueda cambia unos
/// pocos puntos de ratio por un tiempo de compresion acotado.
const MAX_CHAIN: usize = 96;

/// Escribe un simbolo de literal o de fin de bloque con el arbol fijo de deflate
/// (RFC 1951, seccion 3.2.6).
#[inline]
fn write_symbol(bw: &mut BitWriter, simbolo: u16) {
    match simbolo {
        0..=143 => bw.write_code(0x30 + simbolo as u32, 8),
        144..=255 => bw.write_code(0x190 + (simbolo as u32 - 144), 9),
        256..=279 => bw.write_code(simbolo as u32 - 256, 7),
        _ => bw.write_code(0xC0 + (simbolo as u32 - 280), 8),
    }
}

/// Compresion deflate con arboles Huffman fijos y emparejamiento LZ77.
///
/// Se usa el arbol fijo, y no arboles dinamicos, porque evita escribir la
/// descripcion de los arboles y su codificacion de longitudes de codigo: sobre
/// imagenes ya filtradas por PNG la diferencia de tamano es pequena y el codigo
/// que hay que sostener es mucho menor.
pub fn deflate_fixed(datos: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter::new(datos.len() / 2 + 64);
    bw.write_bits(1, 1); // ultimo bloque
    bw.write_bits(1, 2); // arboles Huffman fijos

    let n = datos.len();
    let mut head = vec![u32::MAX; HASH_SIZE];
    let mut prev = vec![u32::MAX; n.max(1)];

    #[inline]
    fn hash3(d: &[u8], p: usize) -> usize {
        (((d[p] as usize) << 10) ^ ((d[p + 1] as usize) << 5) ^ (d[p + 2] as usize))
            & (HASH_SIZE - 1)
    }

    let mut pos = 0usize;
    while pos < n {
        let mut mejor_len = 0usize;
        let mut mejor_dist = 0usize;

        if pos + MIN_MATCH <= n {
            let h = hash3(datos, pos);
            let mut candidato = head[h];
            let limite = pos.saturating_sub(WINDOW);
            let max_len = (n - pos).min(MAX_MATCH);
            let mut intentos = 0;

            while candidato != u32::MAX && intentos < MAX_CHAIN {
                let c = candidato as usize;
                if c < limite {
                    break;
                }
                // Comparar primero el byte que ampliaria la mejor coincidencia
                // descarta la mayoria de los candidatos con una sola lectura.
                if mejor_len == 0 || datos[c + mejor_len] == datos[pos + mejor_len] {
                    let mut l = 0usize;
                    while l < max_len && datos[c + l] == datos[pos + l] {
                        l += 1;
                    }
                    if l > mejor_len {
                        mejor_len = l;
                        mejor_dist = pos - c;
                        if l >= max_len {
                            break;
                        }
                    }
                }
                candidato = prev[c];
                intentos += 1;
            }
        }

        if mejor_len >= MIN_MATCH {
            // Codigo de longitud.
            let mut i = LENGTH_BASE.len() - 1;
            while LENGTH_BASE[i] as usize > mejor_len {
                i -= 1;
            }
            write_symbol(&mut bw, 257 + i as u16);
            if LENGTH_EXTRA[i] > 0 {
                bw.write_bits(
                    (mejor_len - LENGTH_BASE[i] as usize) as u32,
                    LENGTH_EXTRA[i],
                );
            }
            // Codigo de distancia: cinco bits fijos, tambien de mayor a menor.
            let mut j = DIST_BASE.len() - 1;
            while DIST_BASE[j] as usize > mejor_dist {
                j -= 1;
            }
            bw.write_code(j as u32, 5);
            if DIST_EXTRA[j] > 0 {
                bw.write_bits((mejor_dist - DIST_BASE[j] as usize) as u32, DIST_EXTRA[j]);
            }

            // Insertar todas las posiciones cubiertas mantiene la tabla al dia.
            for p in pos..pos + mejor_len {
                if p + MIN_MATCH <= n {
                    let h = hash3(datos, p);
                    prev[p] = head[h];
                    head[h] = p as u32;
                }
            }
            pos += mejor_len;
        } else {
            write_symbol(&mut bw, datos[pos] as u16);
            if pos + MIN_MATCH <= n {
                let h = hash3(datos, pos);
                prev[pos] = head[h];
                head[h] = pos as u32;
            }
            pos += 1;
        }
    }

    write_symbol(&mut bw, 256); // fin de bloque
    bw.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Descompresor minimo de bloques con arbol Huffman fijo.
    ///
    /// Existe solo para las pruebas: cierra el ciclo sobre el compresor y
    /// garantiza que el flujo que se escribe en los PNG es realmente valido, sin
    /// tener que confiar en una herramienta externa.
    fn inflate_fixed(entrada: &[u8]) -> Vec<u8> {
        struct BitReader<'a> {
            datos: &'a [u8],
            pos: usize,
            bit: u32,
        }
        impl BitReader<'_> {
            fn bit(&mut self) -> u32 {
                let b = (self.datos[self.pos] >> self.bit) & 1;
                self.bit += 1;
                if self.bit == 8 {
                    self.bit = 0;
                    self.pos += 1;
                }
                b as u32
            }
            /// Bits sueltos: llegan con el menos significativo primero.
            fn bits(&mut self, n: u32) -> u32 {
                let mut v = 0;
                for i in 0..n {
                    v |= self.bit() << i;
                }
                v
            }
            /// Codigos Huffman: llegan con el mas significativo primero.
            fn code(&mut self, n: u32) -> u32 {
                let mut v = 0;
                for _ in 0..n {
                    v = (v << 1) | self.bit();
                }
                v
            }
        }

        let mut br = BitReader {
            datos: entrada,
            pos: 0,
            bit: 0,
        };
        let mut salida = Vec::new();
        loop {
            let final_bloque = br.bits(1);
            let tipo = br.bits(2);
            assert_eq!(tipo, 1, "solo se emite el arbol fijo");
            loop {
                // Desambiguar por longitud segun los rangos del arbol fijo.
                let v7 = br.code(7);
                let simbolo: u16 = if v7 < 24 {
                    256 + v7 as u16
                } else {
                    let v8 = (v7 << 1) | br.bit();
                    if (0x30..=0xBF).contains(&v8) {
                        (v8 - 0x30) as u16
                    } else if (0xC0..=0xC7).contains(&v8) {
                        280 + (v8 - 0xC0) as u16
                    } else {
                        let v9 = (v8 << 1) | br.bit();
                        144 + (v9 - 0x190) as u16
                    }
                };

                if simbolo == 256 {
                    break;
                }
                if simbolo < 256 {
                    salida.push(simbolo as u8);
                    continue;
                }
                let i = (simbolo - 257) as usize;
                let longitud = LENGTH_BASE[i] as usize + br.bits(LENGTH_EXTRA[i]) as usize;
                let j = br.code(5) as usize;
                let distancia = DIST_BASE[j] as usize + br.bits(DIST_EXTRA[j]) as usize;
                let inicio = salida.len() - distancia;
                for k in 0..longitud {
                    let b = salida[inicio + k];
                    salida.push(b);
                }
            }
            if final_bloque == 1 {
                break;
            }
        }
        salida
    }

    fn imagen_de_prueba(w: usize, h: usize) -> Image {
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                // Degradados suaves mas un patron periodico: material realista
                // para el emparejador LZ77, con zonas planas y zonas con detalle.
                let r = (x * 255 / w.max(1)) as u8;
                let g = (y * 255 / h.max(1)) as u8;
                let b = if (x / 4 + y / 4) % 2 == 0 { 40 } else { 200 };
                img.set(x, y, [r, g, b]);
            }
        }
        img
    }

    #[test]
    fn deflate_reconstruye_exactamente_los_datos() {
        let casos: Vec<Vec<u8>> = vec![
            b"a".to_vec(),
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec(),
            b"abcabcabcabcabcabcabcabc".to_vec(),
            b"La Abadia del Eclipse, la Abadia del Eclipse, la abadia.".to_vec(),
            (0..=255u8).collect(),
            (0..5000).map(|i| (i * 7 % 251) as u8).collect(),
            vec![0u8; 100_000],
        ];
        for datos in casos {
            let comprimido = deflate_fixed(&datos);
            assert_eq!(inflate_fixed(&comprimido), datos, "len={}", datos.len());
        }
    }

    #[test]
    fn deflate_comprime_de_verdad_los_datos_repetitivos() {
        let datos = vec![0u8; 200_000];
        let ratio = datos.len() as f64 / deflate_fixed(&datos).len() as f64;
        assert!(ratio > 50.0, "ratio insuficiente: {ratio}");
    }

    #[test]
    fn el_flujo_zlib_del_png_se_descomprime_a_las_filas_filtradas() {
        let img = imagen_de_prueba(64, 48);
        let filtradas = img.filter_scanlines();
        let zlib = zlib_compress(&filtradas);
        assert_eq!(zlib[0], 0x78);
        assert_eq!((u16::from_be_bytes([zlib[0], zlib[1]])) % 31, 0);
        let recuperado = inflate_fixed(&zlib[2..zlib.len() - 4]);
        assert_eq!(recuperado, filtradas);
        let adler = u32::from_be_bytes([
            zlib[zlib.len() - 4],
            zlib[zlib.len() - 3],
            zlib[zlib.len() - 2],
            zlib[zlib.len() - 1],
        ]);
        assert_eq!(adler, adler32(&filtradas));
    }

    #[test]
    fn el_filtrado_png_es_reversible() {
        let img = imagen_de_prueba(37, 21);
        let filtradas = img.filter_scanlines();
        let stride = img.width * 3;
        let mut anterior = vec![0u8; stride];
        let mut reconstruida = Vec::with_capacity(img.data.len());
        for y in 0..img.height {
            let base = y * (stride + 1);
            let tipo = filtradas[base];
            let mut fila = filtradas[base + 1..base + 1 + stride].to_vec();
            for i in 0..stride {
                let a = if i >= 3 { fila[i - 3] } else { 0 };
                let b = anterior[i];
                let c = if i >= 3 { anterior[i - 3] } else { 0 };
                fila[i] = match tipo {
                    0 => fila[i],
                    1 => fila[i].wrapping_add(a),
                    2 => fila[i].wrapping_add(b),
                    3 => fila[i].wrapping_add(((a as u16 + b as u16) / 2) as u8),
                    _ => fila[i].wrapping_add(paeth(a, b, c)),
                };
            }
            reconstruida.extend_from_slice(&fila);
            anterior = fila;
        }
        assert_eq!(reconstruida, img.data);
    }

    #[test]
    fn la_estructura_del_png_es_valida() {
        let img = imagen_de_prueba(19, 7);
        let png = img.to_png_bytes();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        // Recorrer los trozos comprobando cada CRC.
        let mut i = 8usize;
        let mut tipos = Vec::new();
        while i < png.len() {
            let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
            let tipo = String::from_utf8(png[i + 4..i + 8].to_vec()).unwrap();
            let crc_leido = u32::from_be_bytes([
                png[i + 8 + len],
                png[i + 9 + len],
                png[i + 10 + len],
                png[i + 11 + len],
            ]);
            assert_eq!(crc_leido, crc32(&png[i + 4..i + 8 + len]), "CRC de {tipo}");
            tipos.push(tipo);
            i += 12 + len;
        }
        assert_eq!(i, png.len(), "los trozos deben cubrir el fichero exacto");
        assert_eq!(tipos, vec!["IHDR", "IDAT", "IEND"]);

        // Campos de IHDR.
        assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), 19);
        assert_eq!(u32::from_be_bytes([png[20], png[21], png[22], png[23]]), 7);
        assert_eq!(png[24], 8, "profundidad de bits");
        assert_eq!(png[25], 2, "color verdadero RGB");
    }

    #[test]
    fn crc32_y_adler32_coinciden_con_los_valores_conocidos() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn ppm_binario_da_la_vuelta_completa() {
        let dir = std::env::temp_dir().join("abadia_test_ppm");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("ida_y_vuelta.ppm");
        let img = imagen_de_prueba(23, 11);
        img.write_ppm(&ruta).unwrap();
        let leida = Image::read_ppm(&ruta).unwrap();
        assert_eq!(leida, img);
        std::fs::remove_file(&ruta).ok();
    }

    #[test]
    fn se_lee_ppm_de_texto_con_comentarios() {
        let fuente = b"P3\n# textura de prueba\n2 2\n255\n255 0 0  0 255 0\n0 0 255  9 9 9\n";
        let img = Image::from_ppm_bytes(fuente).unwrap();
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
        assert_eq!(img.get(0, 0), [255, 0, 0]);
        assert_eq!(img.get(1, 0), [0, 255, 0]);
        assert_eq!(img.get(0, 1), [0, 0, 255]);
        assert_eq!(img.get(1, 1), [9, 9, 9]);
    }

    #[test]
    fn se_reescala_el_valor_maximo_no_estandar() {
        let img = Image::from_ppm_bytes(b"P3 1 1 15 15 0 7").unwrap();
        assert_eq!(img.get(0, 0)[0], 255);
        assert_eq!(img.get(0, 0)[1], 0);
        assert!(img.get(0, 0)[2] > 110 && img.get(0, 0)[2] < 125);
    }

    #[test]
    fn los_ppm_corruptos_se_rechazan_con_mensaje() {
        assert!(Image::from_ppm_bytes(b"P7\n1 1\n255\n").is_err());
        assert!(Image::from_ppm_bytes(b"P6\n2 2\n255\nabc").is_err());
        assert!(Image::from_ppm_bytes(b"P6\n0 4\n255\n").is_err());
        assert!(Image::from_ppm_bytes(b"P6\n2 2\n65535\n").is_err());
    }

    #[test]
    fn el_png_de_una_imagen_lisa_es_mucho_menor_que_los_datos_crudos() {
        let mut img = Image::new(256, 256);
        for y in 0..256 {
            for x in 0..256 {
                img.set(x, y, [30, 40, 60]);
            }
        }
        let png = img.to_png_bytes();
        assert!(
            png.len() * 20 < img.data.len(),
            "PNG de {} bytes para {} crudos",
            png.len(),
            img.data.len()
        );
    }
}
