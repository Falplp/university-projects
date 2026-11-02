mod utils;

use crate::utils::Vector2D;
use minifb::{Key, MouseMode, Window, WindowOptions};
use std::cmp::min;
use std::path::Path;
use std::time::Instant;

const WIDTH: usize = 800;
const HEIGHT: usize = 600;

struct SharedShaderInput {
    time: u32,
    width: u32,
    height: u32,
    mouse_row: u32,
    mouse_col: u32,
}

struct Pixel {
    red: u8,
    green: u8,
    blue: u8,
}

impl Pixel {
    fn black() -> Self {
        Self {
            red: 0,
            green: 0,
            blue: 0,
        }
    }

    fn red() -> Self {
        Self {
            red: 255,
            green: 0,
            blue: 0,
        }
    }

    fn purple() -> Self {
        Self {
            red: 255,
            green: 0,
            blue: 255,
        }
    }

    fn as_u32(&self) -> u32 {
        ((self.red as u32) << 16) | ((self.green as u32) << 8) | self.blue as u32
    }
}

fn render_pixel(shader: &Shader, input: &SharedShaderInput, row: u32, col: u32) -> Pixel {
    // TODO:
    // - create a CPU
    // - set `input` values to its memory
    // - run shader on the CPU
    // - if it finished successfully, return a pixel read out of its memory
    // - if it failed, return a purple pixel

    let position = Vector2D::new(row as f32, col as f32);
    let mouse_pos = Vector2D::new(input.mouse_row as f32, input.mouse_col as f32);

    let mut range = (input.time / 10) % 200;
    if range >= 100 {
        range = 200 - range;
    };
    let range = range as f32 / 100.0;

    let circle_size = 10.0 + 30.0 * range;
    let distance = position.distance_euclid(mouse_pos);
    if distance <= circle_size {
        let min_brightness = 20;
        let remainder: u8 = 255 - min_brightness;
        let red = min_brightness + ((1.0 - (distance / circle_size)) * remainder as f32) as u8;
        Pixel {
            red,
            green: 0,
            blue: 0,
        }
    } else {
        Pixel::black()
    }
}

struct Shader();

fn main() {
    let mut framebuffer = vec![0u32; WIDTH * HEIGHT];

    let mut window = Window::new("N-Body Simulation", WIDTH, HEIGHT, WindowOptions::default())
        .expect("failed to create window");

    let mut last_frame = Instant::now();
    let mut time = Instant::now();

    while window.is_open() {
        if window.is_key_down(Key::Escape) || window.is_key_down(Key::Q) {
            break;
        }

        let now = Instant::now();
        let delta = (now - last_frame).as_secs_f32();
        last_frame = now;

        println!("FPS: {:.1}", 1.0 / delta);

        let (mouse_row, mouse_col) = window
            .get_mouse_pos(MouseMode::Clamp)
            .map(|(y, x)| (x as u32, y as u32))
            .unwrap_or((0, 0));
        let elapsed = time.elapsed().as_millis() as u32;
        let input = SharedShaderInput {
            time: elapsed,
            width: WIDTH as u32,
            height: HEIGHT as u32,
            mouse_row,
            mouse_col,
        };

        let shader = load_shader(Path::new("shader.asm"));
        match shader {
            Ok(shader) => {
                framebuffer.fill(0);
                for row in 0..HEIGHT {
                    for col in 0..WIDTH {
                        let pixel = render_pixel(&shader, &input, row as u32, col as u32);
                        framebuffer[row * WIDTH + col] = pixel.as_u32();
                    }
                }
            }
            Err(e) => {
                println!("Error: {e}");
                framebuffer.fill(Pixel::purple().as_u32());
            }
        }

        window
            .update_with_buffer(&framebuffer, WIDTH, HEIGHT)
            .expect("failed to update window");
    }
}

fn load_shader(path: &Path) -> Result<Shader, String> {
    let data = std::fs::read_to_string(path).map_err(|e| "Cannot read file at {path:?}: {e:?}")?;
    // TODO: parse assembly program from `data` and return it in a `Shader`
    Ok(Shader())
}
