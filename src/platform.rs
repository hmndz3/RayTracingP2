//! Ventana nativa de Windows por FFI directo, sin ningun crate.
//!
//! La ventana no participa en el calculo de la imagen. Su unico cometido es
//! presentar en pantalla el framebuffer que ya ha calculado la CPU y recoger las
//! entradas: `StretchDIBits` copia un mapa de bits que vive en memoria principal.
//! No hay contexto grafico, ni shaders, ni ninguna superficie acelerada; la GPU no
//! interviene en la geometria, la iluminacion ni el trazado.
//!
//! # Interpretacion pendiente
//!
//! Este modulo llama a `user32`, `gdi32` y `kernel32`, que son las API del propio
//! sistema operativo y no librerias de terceros. **Queda por confirmar con el
//! profesor** si la restriccion de no usar librerias externas admite esta via. El
//! modo de render a fichero es completamente independiente de este fichero y
//! compila en cualquier sistema, asi que la entrega no depende de esa
//! interpretacion.

#![cfg(windows)]

use crate::camera::{tour_camera, tour_keys, Camera};
use crate::config::Config;
use crate::renderer::{render, RenderSettings, World};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Declaraciones de la API de Windows
// ---------------------------------------------------------------------------

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hdc = *mut c_void;
type Hicon = *mut c_void;
type Hcursor = *mut c_void;
type Hbrush = *mut c_void;
type Wparam = usize;
type Lparam = isize;
type Lresult = isize;
type WndProc = unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult;

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    w_param: Wparam,
    l_param: Lparam,
    time: u32,
    pt: Point,
}

#[repr(C)]
struct WndClassW {
    style: u32,
    lpfn_wnd_proc: Option<WndProc>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: Hinstance,
    h_icon: Hicon,
    h_cursor: Hcursor,
    hbr_background: Hbrush,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
}

#[repr(C)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    bmi_header: BitmapInfoHeader,
    bmi_colors: [u32; 3],
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
    fn UpdateWindow(hwnd: Hwnd) -> i32;
    fn PeekMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> Lresult;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, w: Wparam, l: Lparam) -> Lresult;
    fn PostQuitMessage(code: i32);
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn GetDC(hwnd: Hwnd) -> Hdc;
    fn ReleaseDC(hwnd: Hwnd, dc: Hdc) -> i32;
    fn LoadCursorW(instance: Hinstance, name: *const u16) -> Hcursor;
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
    fn SetCapture(hwnd: Hwnd) -> Hwnd;
    fn ReleaseCapture() -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    #[allow(clippy::too_many_arguments)]
    fn StretchDIBits(
        dc: Hdc,
        x_dest: i32,
        y_dest: i32,
        w_dest: i32,
        h_dest: i32,
        x_src: i32,
        y_src: i32,
        w_src: i32,
        h_src: i32,
        bits: *const c_void,
        info: *const BitmapInfo,
        usage: u32,
        rop: u32,
    ) -> i32;
    fn SetStretchBltMode(dc: Hdc, mode: i32) -> i32;
}

const WS_OVERLAPPED_WINDOW: u32 = 0x00CF_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const CW_USEDEFAULT: i32 = 0x8000_0000_u32 as i32;
const SW_SHOW: i32 = 5;
const PM_REMOVE: u32 = 1;
const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_KEYDOWN: u32 = 0x0100;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_SIZE: u32 = 0x0005;
const WM_ERASEBKGND: u32 = 0x0014;
const DIB_RGB_COLORS: u32 = 0;
const SRCCOPY: u32 = 0x00CC_0020;
const BI_RGB: u32 = 0;
const HALFTONE: i32 = 4;
const IDC_ARROW: u16 = 32512;

// Codigos de tecla virtuales que usa el programa.
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_OEM_PLUS: u32 = 0xBB;
const VK_OEM_MINUS: u32 = 0xBD;
const VK_ADD: u32 = 0x6B;
const VK_SUBTRACT: u32 = 0x6D;

/// Convierte a la cadena terminada en cero que espera la API.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// Cola de eventos
// ---------------------------------------------------------------------------

/// Evento recogido por el procedimiento de ventana.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Evento {
    Tecla(u32),
    Arrastre { dx: i32, dy: i32 },
    Rueda(i32),
    Redimension { ancho: i32, alto: i32 },
    Cerrar,
}

/// El procedimiento de ventana lo invoca Windows desde el hilo del bucle de
/// mensajes, asi que no puede recibir parametros propios. Deja aqui lo que
/// observa y el bucle lo recoge; el cerrojo se toma una vez por evento, no por
/// fotograma.
static EVENTOS: Mutex<VecDeque<Evento>> = Mutex::new(VecDeque::new());
/// Estado del boton izquierdo y ultima posicion, para calcular el arrastre.
static ARRASTRANDO: AtomicBool = AtomicBool::new(false);
static ULTIMO_X: Mutex<i32> = Mutex::new(0);
static ULTIMO_Y: Mutex<i32> = Mutex::new(0);

fn empujar(e: Evento) {
    if let Ok(mut q) = EVENTOS.lock() {
        // Cota de seguridad: si el render se atasca, los eventos no pueden
        // crecer sin limite.
        if q.len() < 512 {
            q.push_back(e);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, w: Wparam, l: Lparam) -> Lresult {
    match msg {
        WM_CLOSE => {
            empujar(Evento::Cerrar);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        // El fondo no se borra: la imagen cubre todo el area de cliente y
        // borrarla antes produciria parpadeo.
        WM_ERASEBKGND => 1,
        WM_KEYDOWN => {
            empujar(Evento::Tecla(w as u32));
            0
        }
        WM_LBUTTONDOWN => {
            ARRASTRANDO.store(true, Ordering::Relaxed);
            *ULTIMO_X.lock().unwrap() = (l & 0xFFFF) as i16 as i32;
            *ULTIMO_Y.lock().unwrap() = ((l >> 16) & 0xFFFF) as i16 as i32;
            SetCapture(hwnd);
            0
        }
        WM_LBUTTONUP => {
            ARRASTRANDO.store(false, Ordering::Relaxed);
            ReleaseCapture();
            0
        }
        WM_MOUSEMOVE => {
            if ARRASTRANDO.load(Ordering::Relaxed) {
                let x = (l & 0xFFFF) as i16 as i32;
                let y = ((l >> 16) & 0xFFFF) as i16 as i32;
                let mut ux = ULTIMO_X.lock().unwrap();
                let mut uy = ULTIMO_Y.lock().unwrap();
                let (dx, dy) = (x - *ux, y - *uy);
                *ux = x;
                *uy = y;
                if dx != 0 || dy != 0 {
                    empujar(Evento::Arrastre { dx, dy });
                }
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((w >> 16) & 0xFFFF) as i16 as i32;
            empujar(Evento::Rueda(delta));
            0
        }
        WM_SIZE => {
            let ancho = (l & 0xFFFF) as i32;
            let alto = ((l >> 16) & 0xFFFF) as i32;
            if ancho > 0 && alto > 0 {
                empujar(Evento::Redimension { ancho, alto });
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

// ---------------------------------------------------------------------------
// Hilo de render
// ---------------------------------------------------------------------------

/// Peticion de render enviada al hilo de trabajo.
struct Encargo {
    camera: Camera,
    settings: RenderSettings,
    /// Numero de peticion. Solo se presenta el resultado de la mas reciente.
    generacion: u64,
    /// Verdadero si es el render de calidad, el que se lanza al detenerse.
    final_: bool,
}

/// Imagen ya presentable.
struct Cuadro {
    ancho: usize,
    alto: usize,
    pixeles: Vec<u32>,
    generacion: u64,
    seconds: f64,
    final_: bool,
}

impl Default for Cuadro {
    fn default() -> Cuadro {
        Cuadro {
            ancho: 1,
            alto: 1,
            pixeles: vec![0],
            generacion: 0,
            seconds: 0.0,
            final_: false,
        }
    }
}

/// Estado compartido entre el hilo de ventana y el de render.
struct Compartido {
    cuadro: Mutex<Cuadro>,
    /// Pide al render en curso que abandone.
    cancelar: AtomicBool,
    /// Hay un cuadro nuevo sin presentar.
    nuevo: AtomicBool,
    /// El hilo de render debe terminar.
    fin: AtomicBool,
}

/// Arranca la ventana interactiva y no vuelve hasta que se cierra.
pub fn run_window(world: World, config: &Config) -> Result<(), String> {
    let ancho_ventana = config.settings.width as i32;
    let alto_ventana = config.settings.height as i32;

    let compartido = Arc::new(Compartido {
        cuadro: Mutex::new(Cuadro::default()),
        cancelar: AtomicBool::new(false),
        nuevo: AtomicBool::new(false),
        fin: AtomicBool::new(false),
    });

    let (tx, rx) = mpsc::channel::<Encargo>();
    let mundo = Arc::new(world);

    // Hilo de render. La ventana nunca calcula un pixel: solo pide y presenta.
    let hilo = {
        let compartido = Arc::clone(&compartido);
        let mundo = Arc::clone(&mundo);
        std::thread::spawn(move || {
            while let Ok(mut encargo) = rx.recv() {
                if compartido.fin.load(Ordering::Relaxed) {
                    break;
                }
                // Si mientras se trabajaba han llegado mas peticiones, las
                // intermedias ya no interesan: se salta directamente a la ultima.
                while let Ok(siguiente) = rx.try_recv() {
                    encargo = siguiente;
                }
                compartido.cancelar.store(false, Ordering::Relaxed);

                let resultado = render(
                    &mundo,
                    &encargo.camera,
                    &encargo.settings,
                    Some(&compartido.cancelar),
                    None,
                );

                let Some(reporte) = resultado else {
                    continue; // cancelado: la camara se movio, no hay nada que mostrar
                };

                let mut pixeles = vec![0u32; encargo.settings.width * encargo.settings.height];
                reporte
                    .framebuffer
                    .to_bgra(encargo.settings.exposure, &mut pixeles);

                if let Ok(mut c) = compartido.cuadro.lock() {
                    // Un resultado viejo no puede pisar a uno mas reciente.
                    if encargo.generacion >= c.generacion {
                        *c = Cuadro {
                            ancho: encargo.settings.width,
                            alto: encargo.settings.height,
                            pixeles,
                            generacion: encargo.generacion,
                            seconds: reporte.seconds,
                            final_: encargo.final_,
                        };
                        compartido.nuevo.store(true, Ordering::Release);
                    }
                }
            }
        })
    };

    let resultado = bucle_ventana(
        &compartido,
        &tx,
        config,
        ancho_ventana,
        alto_ventana,
        mundo.grid.bounds(),
    );

    compartido.fin.store(true, Ordering::Relaxed);
    compartido.cancelar.store(true, Ordering::Relaxed);
    drop(tx);
    let _ = hilo.join();
    resultado
}

/// Calidad de la vista interactiva.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Calidad {
    Baja,
    Media,
    Alta,
}

impl Calidad {
    fn divisor(self) -> usize {
        match self {
            Calidad::Baja => 4,
            Calidad::Media => 3,
            Calidad::Alta => 2,
        }
    }
    fn nombre(self) -> &'static str {
        match self {
            Calidad::Baja => "baja",
            Calidad::Media => "media",
            Calidad::Alta => "alta",
        }
    }
}

#[allow(clippy::too_many_lines)]
fn bucle_ventana(
    compartido: &Arc<Compartido>,
    tx: &mpsc::Sender<Encargo>,
    config: &Config,
    ancho0: i32,
    alto0: i32,
    limites: crate::geometry::Aabb,
) -> Result<(), String> {
    unsafe {
        let instancia = GetModuleHandleW(std::ptr::null());
        let clase = wide("AbadiaDelEclipse");
        let wc = WndClassW {
            style: 0x0003, // CS_HREDRAW | CS_VREDRAW
            lpfn_wnd_proc: Some(wnd_proc),
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: instancia,
            h_icon: std::ptr::null_mut(),
            h_cursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW as *const u16),
            hbr_background: std::ptr::null_mut(),
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: clase.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            return Err("no se pudo registrar la clase de ventana".into());
        }

        let titulo = wide("La Abadia del Eclipse");
        let hwnd = CreateWindowExW(
            0,
            clase.as_ptr(),
            titulo.as_ptr(),
            WS_OVERLAPPED_WINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            ancho0,
            alto0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instancia,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return Err("no se pudo crear la ventana".into());
        }
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        println!();
        println!("Controles:");
        println!("  flechas o arrastrar        girar la camara");
        println!("  rueda, + y -               acercar y alejar");
        println!("  R                          restablecer la vista inicial");
        println!("  1 2 3                      calidad de la vista interactiva");
        println!("  N                          activar o desactivar los mapas normales");
        println!("  T                          recorrido automatico");
        println!("  P                          guardar una captura");
        println!("  Esc o Q                    salir");
        println!();

        let mut camara = config.camera;
        camara.enforce_outside(limites, 1.5);
        let mut calidad = Calidad::Media;
        let mut normales = config.settings.normal_maps;
        let mut recorrido = false;
        let mut recorrido_t = 0.0f64;
        let keys = tour_keys();

        let mut cliente = (ancho0, alto0);
        let mut generacion = 1u64;
        let mut pendiente_final = false;
        let mut ultimo_movimiento = Instant::now();
        let mut capturas = 0u32;
        let mut ultimo_titulo = Instant::now();
        let mut info = String::new();

        // Peticion inicial.
        let pedir = |camara: Camera,
                     cliente: (i32, i32),
                     calidad: Calidad,
                     normales: bool,
                     generacion: &mut u64,
                     final_: bool| {
            let (w, h) = if final_ {
                (cliente.0.max(1) as usize, cliente.1.max(1) as usize)
            } else {
                let d = calidad.divisor();
                (
                    (cliente.0.max(1) as usize / d).max(1),
                    (cliente.1.max(1) as usize / d).max(1),
                )
            };
            let settings = RenderSettings {
                width: w,
                height: h,
                samples: if final_ { config.settings.samples } else { 1 },
                max_depth: if final_ { config.settings.max_depth } else { 3 },
                exposure: config.settings.exposure,
                normal_maps: normales,
                threads: config.settings.threads,
                tile: config.settings.tile,
            };
            *generacion += 1;
            compartido.cancelar.store(true, Ordering::Relaxed);
            let _ = tx.send(Encargo {
                camera: camara,
                settings,
                generacion: *generacion,
                final_,
            });
        };

        pedir(camara, cliente, calidad, normales, &mut generacion, false);

        let mut msg = std::mem::zeroed::<Msg>();
        let mut salir = false;
        while !salir {
            // Bombear todos los mensajes pendientes sin bloquear, para que la
            // interfaz siga respondiendo mientras el render trabaja.
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == 0x0012 {
                    // WM_QUIT
                    salir = true;
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if salir {
                break;
            }

            // Procesar lo que haya dejado el procedimiento de ventana.
            let eventos: Vec<Evento> = {
                let mut q = EVENTOS.lock().unwrap();
                q.drain(..).collect()
            };
            let mut cambio = false;
            for e in eventos {
                match e {
                    Evento::Cerrar => salir = true,
                    Evento::Redimension { ancho, alto } => {
                        cliente = (ancho, alto);
                        cambio = true;
                    }
                    Evento::Arrastre { dx, dy } => {
                        camara.orbit(dx as f64 * 0.32, -dy as f64 * 0.28);
                        recorrido = false;
                        cambio = true;
                    }
                    Evento::Rueda(delta) => {
                        let pasos = delta as f64 / 120.0;
                        camara.zoom(0.88f64.powf(pasos));
                        cambio = true;
                    }
                    Evento::Tecla(vk) => match vk {
                        VK_ESCAPE | 0x51 => salir = true, // Esc, Q
                        VK_LEFT => {
                            camara.orbit(-4.0, 0.0);
                            recorrido = false;
                            cambio = true;
                        }
                        VK_RIGHT => {
                            camara.orbit(4.0, 0.0);
                            recorrido = false;
                            cambio = true;
                        }
                        VK_UP => {
                            camara.orbit(0.0, 3.0);
                            recorrido = false;
                            cambio = true;
                        }
                        VK_DOWN => {
                            camara.orbit(0.0, -3.0);
                            recorrido = false;
                            cambio = true;
                        }
                        VK_OEM_PLUS | VK_ADD => {
                            camara.zoom(0.9);
                            cambio = true;
                        }
                        VK_OEM_MINUS | VK_SUBTRACT => {
                            camara.zoom(1.0 / 0.9);
                            cambio = true;
                        }
                        0x52 => {
                            // R
                            camara = Camera::initial();
                            recorrido = false;
                            cambio = true;
                        }
                        0x4E => {
                            // N
                            normales = !normales;
                            println!(
                                "mapas normales: {}",
                                if normales {
                                    "activados"
                                } else {
                                    "desactivados"
                                }
                            );
                            cambio = true;
                        }
                        0x54 | VK_SPACE => {
                            // T o espacio
                            recorrido = !recorrido;
                            println!(
                                "recorrido automatico: {}",
                                if recorrido { "en marcha" } else { "detenido" }
                            );
                            cambio = true;
                        }
                        0x31 => {
                            calidad = Calidad::Baja;
                            println!("calidad de vista: {}", calidad.nombre());
                            cambio = true;
                        }
                        0x32 => {
                            calidad = Calidad::Media;
                            println!("calidad de vista: {}", calidad.nombre());
                            cambio = true;
                        }
                        0x33 => {
                            calidad = Calidad::Alta;
                            println!("calidad de vista: {}", calidad.nombre());
                            cambio = true;
                        }
                        0x50 => {
                            // P
                            if let Ok(c) = compartido.cuadro.lock() {
                                capturas += 1;
                                let ruta = format!("captura_{capturas:03}.png");
                                match guardar_captura(&c, &ruta) {
                                    Ok(()) => println!("captura guardada en {ruta}"),
                                    Err(e) => eprintln!("no se pudo guardar la captura: {e}"),
                                }
                            }
                        }
                        _ => {}
                    },
                }
            }

            if recorrido {
                recorrido_t = (recorrido_t + 0.0016) % 1.0;
                camara = tour_camera(&keys, recorrido_t, config.camera.vfov);
                cambio = true;
            }

            if cambio {
                camara.enforce_outside(limites, 1.5);
                ultimo_movimiento = Instant::now();
                pendiente_final = true;
                pedir(camara, cliente, calidad, normales, &mut generacion, false);
            }

            // Calidad adaptativa: cuando la camara lleva un momento quieta se
            // pide el render a resolucion completa. Si vuelve a moverse, ese
            // trabajo se cancela y se descarta.
            if pendiente_final
                && !recorrido
                && ultimo_movimiento.elapsed() > Duration::from_millis(420)
            {
                pendiente_final = false;
                pedir(camara, cliente, calidad, normales, &mut generacion, true);
            }

            // Presentar el ultimo cuadro disponible.
            if compartido.nuevo.swap(false, Ordering::Acquire) {
                if let Ok(c) = compartido.cuadro.lock() {
                    presentar(hwnd, &c, cliente);
                    info = format!(
                        "{}x{} {} | {:.0} ms | {} | mapas normales {}",
                        c.ancho,
                        c.alto,
                        if c.final_ { "calidad" } else { "vista" },
                        c.seconds * 1000.0,
                        calidad.nombre(),
                        if normales { "si" } else { "no" }
                    );
                }
            }

            if ultimo_titulo.elapsed() > Duration::from_millis(250) && !info.is_empty() {
                ultimo_titulo = Instant::now();
                let t = wide(&format!("La Abadia del Eclipse  -  {info}"));
                SetWindowTextW(hwnd, t.as_ptr());
            }

            // Sin nada que hacer, ceder la CPU: el hilo de render la necesita
            // entera y el bucle de mensajes no debe girar en vacio.
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    Ok(())
}

/// Copia el framebuffer al area de cliente.
///
/// `bi_height` negativo indica que la primera fila del bufer es la de arriba, que
/// es el orden en el que el trazador escribe. Sin ese signo la imagen saldria del
/// reves.
unsafe fn presentar(hwnd: Hwnd, cuadro: &Cuadro, cliente: (i32, i32)) {
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: cliente.0,
        bottom: cliente.1,
    };
    GetClientRect(hwnd, &mut rect);
    let ancho = (rect.right - rect.left).max(1);
    let alto = (rect.bottom - rect.top).max(1);

    let info = BitmapInfo {
        bmi_header: BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            bi_width: cuadro.ancho as i32,
            bi_height: -(cuadro.alto as i32),
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: BI_RGB,
            bi_size_image: 0,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        },
        bmi_colors: [0; 3],
    };

    let dc = GetDC(hwnd);
    if dc.is_null() {
        return;
    }
    // Interpolacion al ampliar: la vista interactiva se calcula a resolucion
    // reducida y sin esto se veria a bloques.
    SetStretchBltMode(dc, HALFTONE);
    StretchDIBits(
        dc,
        0,
        0,
        ancho,
        alto,
        0,
        0,
        cuadro.ancho as i32,
        cuadro.alto as i32,
        cuadro.pixeles.as_ptr() as *const c_void,
        &info,
        DIB_RGB_COLORS,
        SRCCOPY,
    );
    ReleaseDC(hwnd, dc);
}

/// Guarda el cuadro presentado como PNG.
fn guardar_captura(cuadro: &Cuadro, ruta: &str) -> std::io::Result<()> {
    let mut img = crate::image::Image::new(cuadro.ancho, cuadro.alto);
    for y in 0..cuadro.alto {
        for x in 0..cuadro.ancho {
            let v = cuadro.pixeles[y * cuadro.ancho + x];
            img.set(
                x,
                y,
                [
                    ((v >> 16) & 0xFF) as u8,
                    ((v >> 8) & 0xFF) as u8,
                    (v & 0xFF) as u8,
                ],
            );
        }
    }
    img.save(ruta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_cadena_ancha_termina_en_cero() {
        let w = wide("abc");
        assert_eq!(w, vec![97, 98, 99, 0]);
        assert_eq!(*wide("").last().unwrap(), 0);
    }

    #[test]
    fn las_estructuras_tienen_el_tamano_que_espera_la_api() {
        // Si el compilador anadiese relleno, la API leeria basura.
        assert_eq!(std::mem::size_of::<BitmapInfoHeader>(), 40);
        assert_eq!(std::mem::size_of::<Point>(), 8);
        assert_eq!(std::mem::size_of::<Rect>(), 16);
    }

    #[test]
    fn la_cola_de_eventos_esta_acotada() {
        EVENTOS.lock().unwrap().clear();
        for _ in 0..2000 {
            empujar(Evento::Rueda(120));
        }
        let n = EVENTOS.lock().unwrap().len();
        assert!(n <= 512, "la cola crecio sin limite: {n}");
        EVENTOS.lock().unwrap().clear();
    }

    #[test]
    fn los_divisores_de_calidad_son_crecientes() {
        assert!(Calidad::Baja.divisor() > Calidad::Media.divisor());
        assert!(Calidad::Media.divisor() > Calidad::Alta.divisor());
        assert!(Calidad::Alta.divisor() >= 1);
    }

    #[test]
    fn el_cuadro_por_omision_es_presentable() {
        let c = Cuadro::default();
        assert_eq!(c.pixeles.len(), c.ancho * c.alto);
        assert!(c.ancho > 0 && c.alto > 0);
    }

    #[test]
    fn la_captura_conserva_los_canales() {
        let cuadro = Cuadro {
            ancho: 2,
            alto: 1,
            pixeles: vec![0x00FF_8040, 0x0001_0203],
            generacion: 1,
            seconds: 0.0,
            final_: true,
        };
        let dir = std::env::temp_dir().join("abadia_captura_test.png");
        guardar_captura(&cuadro, dir.to_str().unwrap()).unwrap();
        assert!(dir.exists());
        std::fs::remove_file(&dir).ok();
    }
}
