# Estado del proyecto

Documento de trabajo para retomar el desarrollo. Se retirara cuando la entrega
este cerrada.

Ultima actualizacion: sesion 2, tras el commit `feat(scene)`.

## Situacion actual

Diecisiete commits, arbol limpio, **nada subido todavia a GitHub** (`git push -u
origin main` cuando se quiera publicar).

```bash
cargo test && cargo clippy --all-targets && cargo build --release
```

- `cargo clippy --all-targets`: sin advertencias.
- `cargo test`: **195 pruebas, 6 fallan**, todas en `scene` y todas de
  composicion. El resto del proyecto esta en verde.
- Recursos generados y versionados en `assets/`. Para regenerarlos:
  `cargo run --release -- textures --previews docs/images/textures`

## Lo primero al retomar: las seis pruebas de `scene`

Son defectos reales de colocacion, no de las pruebas. Diagnostico hecho:

1. **`no_hay_bloques_flotando`** y **`no_hay_bloques_flotando_con_ninguna_semilla`**
   — Bloques sueltos en `[22,24,17]`, `[18,24,19]`, `[18,24,21]`. En `torre()`,
   el fuste se levanta hasta `y0+15` pero la coronacion escribe escombro en
   `tope`, que puede llegar a `y0+17`. **Arreglo**: subir el fuste a `y0+18` y
   recortar `tope` al rango realmente construido.
2. **`la_torre_es_el_elemento_mas_alto_y_esta_rota`** — la torre llega a 24 y el
   hastial de la fachada a 23. **Arreglo**: el mismo de arriba; con el fuste mas
   alto la torre vuelve a destacar.
3. **`la_portada_esta_abierta_y_tiene_arco`** — ya corregido el orden (primero el
   vano rectangular, luego `carve_arch_z` desde `y0+4`), falta volver a ejecutar
   y comprobar que la columna del eje abre mas que las laterales.
4. **`la_pasarela_cruza_el_agua_y_se_apoya_en_pilotes`** — el estanque era
   demasiado somero en la linea de la pasarela. Ya se subio el radio a 5.4 y la
   profundidad a 3.8 y se movio la pasarela a `z = 5`; falta comprobar.
5. **`el_camino_llega_de_la_orilla_a_la_portada`** — el reborde de la isla baja
   el terreno del borde por debajo del plano del agua y `camino()` salta esas
   columnas. Ya se movio el arranque de la ruta a `z = 3`; falta comprobar.

Tras arreglarlas hay que **renderizar y mirar la imagen**, que es el paso que
todavia no se ha dado.

## Modulos terminados

`math`, `ray`, `geometry`, `camera`, `image`, `noise`, `texture`, `skybox`,
`texgen`, `material`, `acceleration`, `lighting`, `renderer`, `terrain`.

`scene` esta escrito y compila, con las seis pruebas pendientes de arriba.

Lo mas sustancial que ya funciona:

- Rejilla voxel con DDA que fusiona caras entre celdas del mismo medio, validada
  contra una busqueda exhaustiva independiente sobre siete mil rayos.
- Trazado recursivo con Fresnel, reflexion, refraccion, reflexion interna total,
  medios con Beer-Lambert y reparto de energia.
- Render por bloques con hilos de la biblioteca estandar, reparto dinamico,
  cancelacion y resultado independiente del numero de hilos.
- Sombras que devuelven transmitancia, no un booleano, de modo que el vitral
  proyecta luz de color.
- Emisores agrupados por contigueidad y muestreados por importancia.
- Oclusion de contacto entre bloques, sin lanzar rayos.
- Terreno procedural de 24 x 24 con semilla reproducible.
- Escritor PNG propio, verificado con un descompresor escrito en las pruebas.

## Lo que falta

1. Arreglar las seis pruebas de `scene` y **mirar el primer render**.
2. Ajustar composicion, exposicion y materiales sobre lo que se vea.
3. `platform`: ventana Win32 por FFI, presentacion con `StretchDIBits`, entradas
   y calidad adaptativa.
4. CLI completa: `render`, `window`, `tour`, `benchmark`.
5. Pruebas de integracion en `tests/`, mediciones reales de rendimiento.
6. Capturas, recorrido demostrativo y README.

## Disposicion del diorama

Rejilla de 24 x 30 x 24. La camara arranca en `yaw -152`, es decir en la esquina
de `x` y `z` bajos, de modo que ve las caras `-X` y `-Z`.

| Elemento | Huella |
| --- | --- |
| Nave | `x 10..17`, `z 12..20`, suelo en `y = 7` |
| Fachada con vitral | plano `z = 12`; vidrio en `x 11..15`, `y 13..18` |
| Torre | `x 18..22`, `z 16..21` |
| Estanque | centro `(9, 6)`, radio 5.4, plano del agua `y = 6` |
| Pasarela | `z = 5`, de `x 4` a `x 15`, tablero en `y = 7` |
| Camposanto | `x 3..6`, `z 12..20` |

## Decisiones ya tomadas

- **Rejilla voxel con DDA en lugar de BVH**, justificado en la cabecera de
  `src/acceleration.rs`.
- **Fusion de medios iguales en el recorrido**: una cara entre dos celdas del
  mismo material no es superficie, lo que evita interfaces falsas en el estanque
  y en el vitral.
- **El medio viaja en la recursion**, no se deduce de la cara impactada.
- **Texturas como recurso versionado**, no como patron evaluado al vuelo.
- **PNG propio** porque el README necesita capturas visibles.
- **Sol a azimut -110 y elevacion 14**: ilumina de frente las caras `-X` y roza
  las `-Z` y las `+Y`, que son las visibles. Ese rasado es lo que hace legibles
  los mapas normales.
- **El relleno no comparte direccion con la luna**: viene de arriba y del lado
  del espectador, que es donde hace falta.

## Punto abierto que hay que confirmar

La ventana interactiva se piensa resolver con FFI directo a `user32`, `gdi32` y
`kernel32`, sin ningun crate. **Queda por confirmar con el profesor** si la
restriccion de no usar librerias externas admite llamar a las API del sistema
operativo. El modo de render a fichero es independiente y portable, asi que la
entrega no depende de esa interpretacion.

## Nota sobre las fechas del historial

El cronograma asignado va del 24 al 29 de septiembre de 2026. Las fechas de autor
de los commits siguen ese cronograma como etiqueta organizativa; la fecha de
committer es la real de ejecucion. Las fechas de autor no son prueba de trabajo
realizado en esos dias.
