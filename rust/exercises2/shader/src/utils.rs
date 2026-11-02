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

    pub fn distance_euclid(&self, other: Vector2D) -> f32 {
        let x = self.x - other.x;
        let y = self.y - other.y;
        ((x * x) + (y * y)).sqrt()
    }
    pub fn distance_manhattan(&self, other: Vector2D) -> f32 {
        let x = self.x - other.x;
        let y = self.y - other.y;
        x.abs() + y.abs()
    }
}
