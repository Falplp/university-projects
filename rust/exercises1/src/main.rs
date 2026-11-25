mod utils;

use minifb::{Key, MouseMode, Window, WindowOptions};
use rand::Rng;
use std::time::Instant;
use utils::{Color, Vector2D, draw_circle};

const WIDTH: usize = 800;
const HEIGHT: usize = 600;

// Gravitational constant, how strongly are planets pulled towards one another
const G: f32 = 10.0;

// How quickly the sun follows the mouse
const SUN_FOLLOW_SPEED: f32 = 0.05;

// Total number of planets created, excluding the sun
const PLANET_COUNT: usize = 100;

#[derive(Copy, Clone)]
struct Body {
    position: Vector2D,
    velocity: Vector2D,
    mass: f32,
    radius: f32,
    color: Color,
}


struct Point {
    x: f32,
    y: f32,
    color: u32,
}


fn draw_voronoi(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    points: &[Point],
) {
    for y in 0..height {
        for x in 0..width {
            let mut closest_distance = f32::MAX;
            let mut color = 0;

            for point in points {
                let dx = x as f32 - point.x;
                let dy = y as f32 - point.y;

                let distance = dx * dx + dy * dy;

                if distance < closest_distance {
                    closest_distance = distance;
                    color = point.color;
                }
            }

            buffer[y * width + x] = color;
        }
    }
}










fn create_planets() -> Vec<Body> {
    // Planets spawn at a random distance from the sun within this range
    const ORBIT_MIN_RADIUS: f32 = 60.0;
    const ORBIT_MAX_RADIUS: f32 = 280.0;

    const SUN_START_POS: Vector2D = Vector2D { x: 400.0, y: 300.0 };
    const SUN_MASS: f32 = 10000.0;

    let mut bodies = Vec::new();

    // The "sun"
    bodies.push(Body {
        position: SUN_START_POS,
        velocity: Vector2D::zero(),
        mass: SUN_MASS,
        radius: 20.0,
        color: Color(0xFFDD33),
    });

    let mut rng = rand::rng();
    for _ in 0..PLANET_COUNT {
        // Pick a random point on a ring around the sun.
        let angle = rng.random_range(0.0..std::f32::consts::TAU);
        let orbit_radius = rng.random_range(ORBIT_MIN_RADIUS..ORBIT_MAX_RADIUS);
        let offset = Vector2D::new(angle.cos(), angle.sin()).scale(orbit_radius);
        let pos = SUN_START_POS.add(offset);

        // Give it a velocity perpendicular to the radius, sized so the sun's
        // gravity roughly balances into a circular orbit at that distance.
        let orbital_speed = (G * SUN_MASS / orbit_radius).sqrt();
        let vel = Vector2D::new(-angle.sin(), angle.cos()).scale(orbital_speed);

        let mass = rng.random_range(5.0..200.0);
        let radius = mass / rng.random_range(20.0..30.0);
        bodies.push(Body {
            position: pos,
            velocity: vel,
            mass,
            radius,
            color: Color(rng.random_range(0x808080..0xFFFFFF)),
        });
    }

    bodies
}

fn compute_nbody(bodies: &mut [Body], delta: f32) {
    let n = bodies.len();
    let mut accelerations = vec![Vector2D::zero(); n];

    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            let direction_vec = bodies[j].position.sub(bodies[i].position);
            // Ensure that the direction is not too small, to avoid planets spinning in a loop
            // or shooting away.
            let distance = direction_vec.length().max(5.0);
            let force = G * bodies[j].mass / (distance * distance);

            accelerations[i] = accelerations[i].add(direction_vec.scale(force / distance));
        }
    }
    for i in 0..n {
        bodies[i].velocity.x += accelerations[i].x * delta;
        bodies[i].velocity.y += accelerations[i].y * delta;

        bodies[i].position.x += bodies[i].velocity.x * delta;
        bodies[i].position.y += bodies[i].velocity.y * delta;
    }

    // TODO: iterate through bodies and accelerations.
    // - Apply acceleration to the velocity of each body
    // - Apply velocity to the position of each body
    // Don't forget to multiply accelerations and velocity by `delta`
}

fn main() {
    let mut planets = create_planets();
    let mut framebuffer = vec![0u32; WIDTH * HEIGHT];

    let mut window = Window::new("N-Body Simulation", WIDTH, HEIGHT, WindowOptions::default())
        .expect("failed to create window");

    let mut last_frame = Instant::now();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let now = Instant::now();
        let delta = (now - last_frame).as_secs_f32();
        last_frame = now;

        // Compute forces on the planets and move them
//         compute_nbody(&mut planets, delta);
        draw_voronoi(&mut buffer, WIDTH, HEIGHT, &points);


        // TODO: Make the sun follow the mouse cursor
        if let Some((mouse_x, mouse_y)) = window.get_mouse_pos(MouseMode::Clamp) {
            let target = Vector2D::new(mouse_x, mouse_y);
            let speed = (SUN_FOLLOW_SPEED * delta).min(1.0);
            planets[0].velocity = Vector2D::zero();

//             let direction = target.sub(planets[0].position);
//             let direction = direction.scale(1.0/direction.length();
//             let direction = direction.scale(speed);

//             planets[0].position = target;

            planets[0].position =   planets[0].position.add(direction);
            // Update the position of the sun, so that it moves towards the sun
        }



        draw_voronoi(&mut buffer, WIDTH, HEIGHT, &points);



        // Reset the framebuffer and draw the planets
//         framebuffer.fill(0);
//         for body in &planets {
//             draw_circle(
//                 &mut framebuffer,
//                 WIDTH,
//                 HEIGHT,
//                 body.position.x,
//                 body.position.y,
//                 body.radius,
//                 body.color,
//             );
//         }

        framebuffer.fill(0);
        for body in &planets {
            draw_circle(
                &mut framebuffer,
                WIDTH,
                HEIGHT,
                body.position.x,
                body.position.y,
                body.radius,
                body.color,
            );
        }

        // Update the window
        window
            .update_with_buffer(&framebuffer, WIDTH, HEIGHT)
            .expect("failed to update window");










    }
}
