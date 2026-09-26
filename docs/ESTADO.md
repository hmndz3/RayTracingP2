# Estado del proyecto

Documento de trabajo para retomar el desarrollo. Se actualiza al final de cada
sesion y se retirara cuando la entrega este cerrada.

Ultima actualizacion: sesion 1, tras el commit `feat(acceleration)`.

## Situacion actual

El proyecto compila, pasa `cargo fmt`, `cargo test` (123 pruebas) y
`cargo clippy --all-targets` sin una sola advertencia. No hay ninguna dependencia
externa declarada.

```bash
cargo test && cargo clippy --all-targets && cargo build --release
```

Los recursos graficos ya estan generados y versionados en `assets/`. Para
regenerarlos:

```bash
cargo run --release -- textures --previews docs/images/textures
```

## Modulos terminados

| Modulo | Contenido |
| --- | --- |
| `math` | Vectores f64, base ortonormal, reflexion, Snell con deteccion de reflexion interna total, Fresnel de Schlick, hash reproducible y generador xorshift |
| `ray` | Rayo con reciproco precalculado, intervalos, medio con indice y absorcion, desplazamiento del origen secundario |
| `geometry` | AABB con test de rebanadas que devuelve caras, UV por cara y tabla de tangentes derecha, registro de impacto |
| `camera` | Orbitador con topes, zoom multiplicativo, empuje fuera del volumen de la escena, recorrido guiado de nueve evidencias |
| `image` | PPM P6/P3 de ida y vuelta, escritor PNG completo con filtrado adaptativo, deflate Huffman fijo + LZ77, CRC-32 y Adler-32 |
| `noise` | Ruido de valor 2D/3D, fbm, crestado, celular y direccional |
| `texture` | Muestreo por UV, sRGB a lineal, cercano y bilineal, lectura de mapas normales, coleccion por nombre |
| `skybox` | Cielo analitico del anochecer, cubemap de seis caras, muestreo por direccion, generacion de caras sin costura |
| `texgen` | Doce texturas originales mas siete mapas normales y las seis caras del cielo |
| `material` | Doce materiales con parametros fisicos, mapeo por bloque y mapeo de ventana para el vitral, normal de sombreado |
| `acceleration` | Rejilla voxel densa, recorrido DDA con fusion de medios iguales, busqueda exhaustiva de referencia |

## Lo que falta

En orden de dependencia:

1. `lighting`: luz principal direccional, relleno frio, ambiente del cubemap,
   sombras y **muestreo explicito de los bloques emisivos** (seleccion de los K
   emisores mas cercanos por importancia y muestras estratificadas).
2. `renderer`: trazado recursivo con reflexion, refraccion, Fresnel, medios y
   Beer-Lambert; control de energia; tono y gamma; render por bloques con hilos
   de la biblioteca estandar y cancelacion.
3. `terrain`: terreno procedural de 24 x 24 con semilla configurable, capas,
   depresion del estanque, meseta de cimientos, vegetacion y escombros.
4. `scene`: abadia, torre derrumbada, arcos, columnas, contrafuertes, estanque,
   puente, camino, faroles, altar, vitral y placa metalica. Validacion de que
   ningun bloque queda flotando.
5. `platform`: ventana Win32 por FFI directo, presentacion del framebuffer con
   `StretchDIBits`, entradas y calidad adaptativa.
6. CLI completa: `render`, `window`, `tour`, `benchmark`.
7. Pruebas de integracion en `tests/`, mediciones reales de rendimiento,
   capturas y README.

## Decisiones ya tomadas

- **Rejilla voxel con DDA en lugar de BVH.** Justificado en la cabecera de
  `src/acceleration.rs`: consulta O(1), sin coste de construccion, sin
  solapamiento y recorrido en orden de distancia.
- **Fusion de medios iguales en el recorrido.** Una cara entre dos celdas del
  mismo material no es superficie, de modo que el estanque y el vitral se
  comportan como un unico cuerpo y no aparecen interfaces falsas.
- **El medio viaja en la recursion**, no se deduce de la cara impactada. Es lo
  que permite distinguir entrada y salida de un volumen transparente.
- **Texturas como recurso, no como patron evaluado al vuelo.** El generador
  escribe PPM al repositorio y el trazador solo lee.
- **PNG propio** porque el README necesita capturas visibles y no se pueden usar
  crates.
- **Direccion de la luz principal**: azimut -70 grados, elevacion 14. Ilumina de
  frente las caras `-X` y roza las `+Y` y `+Z`, que son las visibles desde la
  vista inicial; ese rasado es lo que hace legible el relieve de los mapas
  normales.
- **Geometria del diorama**: rejilla de 24 x 30 x 24. Terreno de 24 x 24 celdas,
  abadia al fondo y a un lado, estanque en primer plano.

## Punto abierto que hay que confirmar

La ventana interactiva se piensa resolver con FFI directo a `user32`, `gdi32` y
`kernel32`, sin ningun crate, presentando en pantalla el framebuffer calculado en
CPU. **Queda por confirmar con el profesor** si la restriccion de no usar
librerias externas admite llamar a las API del sistema operativo por FFI. El modo
de render a fichero es independiente de la ventana y compila en cualquier
sistema, asi que la entrega no depende de esa interpretacion.

## Nota sobre las fechas del historial

El cronograma asignado va del 24 al 29 de septiembre de 2026. Las fechas de autor
de los commits siguen ese cronograma como etiqueta organizativa; la fecha de
committer es la real de ejecucion. Las fechas de autor no son prueba de trabajo
realizado en esos dias.
