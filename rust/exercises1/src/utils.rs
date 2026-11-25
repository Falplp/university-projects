#[derive(Clone, Copy)]
pub struct Vector2D {
    pub x: f32,
    pub y: f32,
}

impl Vector2D {
    pub fn new(x: f32, y: f32) -> Self {
        Vector2D { x, y }
    }

    pub fn zero() -> Self {
        Vector2D::new(0.0, 0.0)
    }

    pub fn sub(self, other: Vector2D) -> Vector2D {
        Vector2D::new(self.x - other.x, self.y - other.y)
    }

    pub fn add(self, other: Vector2D) -> Vector2D {
        Vector2D::new(self.x + other.x, self.y + other.y)
    }

    pub fn scale(self, factor: f32) -> Vector2D {
        Vector2D::new(self.x * factor, self.y * factor)
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

#[derive(Clone, Copy)]
pub struct Color(pub u32);

// Plots every pixel inside the circle into the frame buffer.
pub fn draw_circle(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    cx: f32,
    cy: f32,
    radius: f32,
    color: Color,
) {
    let min_x = (cx - radius).max(0.0) as usize;
    let max_x = (cx + radius).min(width as f32 - 1.0) as usize;
    let min_y = (cy - radius).max(0.0) as usize;
    let max_y = (cy + radius).min(height as f32 - 1.0) as usize;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= radius * radius {
                buffer[y * width + x] = color.0;
            }
        }
    }
}
