//! Run this file with `cargo test --test 04_drawable`.

//! TODO: Implement a function called `draw` that will render a set of drawable shapes onto a string
//! canvas.
//!
//! The `draw` function will receive:
//! - The height of the canvas
//! - The width of the canvas
//! - A slice of things that can be drawn
//!
//! Think about the signature of the function. What approach should be used to work with a set of
//! drawable types? Generics or runtime polymorphism (v-tables)?
//!
//! Implement two geometrical shapes that can be drawn (`Rectangle` and `Circle`).
//! `Rectangle::new` will receive the top left corner of the rectangle, its width, height and the
//! pixels that it contains (in this order).
//! `Circle::new` will receive the center of the circle, its radius and the pixels that it contains
//! (in this order).
//!
//! The coordinate system has its top-left coordinate at (0, 0), rows grow down and columns
//! grow to the right.

#[derive(Copy, Clone)]
struct Point {
    row: u32,
    col: u32,
}

impl Point {
    fn from_row_col(row: u32, col: u32) -> Self {
        Self { row, col }
    }
}

trait Drawable {
    /// If `point` is contained within this drawable object, returns the pixel that should be drawn.
    /// Otherwise, returns `None`.
    ///
    /// Use ONLY integer coordinates for all calculations in this function!
    fn pixel_at(&self, point: Point) -> Option<char>;
}

struct Rectangle {
    corner: Point,
    width: u32,
    height: u32,
    pixel: char,
}
impl Rectangle {
    fn new(corner: Point, width: u32, height: u32, pixel: char) -> Self {
        Self {
            corner,
            width,
            height,
            pixel,
        }
    }
}
impl Drawable for Rectangle {
    fn pixel_at(&self, point: Point) -> Option<char> {
        (point.row >= self.corner.row
            && point.col >= self.corner.col
            && point.row - self.corner.row < self.height
            && point.col - self.corner.col < self.width)
            .then_some(self.pixel)
    }
}
struct Circle {
    center: Point,
    radius: u32,
    pixel: char,
}
impl Circle {
    fn new(center: Point, radius: u32, pixel: char) -> Self {
        Self {
            center,
            radius,
            pixel,
        }
    }
}
impl Drawable for Circle {
    fn pixel_at(&self, point: Point) -> Option<char> {
        let dx = u128::from(point.col.abs_diff(self.center.col));
        let dy = u128::from(point.row.abs_diff(self.center.row));
        // Celočíselná vzdálenost zaokrouhlená dolů odpovídá rastru v zadání.
        ((dx * dx + dy * dy).isqrt() <= u128::from(self.radius)).then_some(self.pixel)
    }
}

// Trait objekty umožňují kombinovat různé tvary v jednom řezu.
fn draw(height: u32, width: u32, shapes: &[&dyn Drawable]) -> String {
    let mut canvas = String::new();
    for row in 0..height {
        for col in 0..width {
            let point = Point::from_row_col(row, col);
            let pixel = shapes.iter().rev().find_map(|shape| shape.pixel_at(point));
            canvas.push(pixel.unwrap_or('.'));
        }
        canvas.push('\n');
    }
    canvas
}

/// Below you can find a set of unit tests.
/// The tests use the [`insta`](https://insta.rs/) snapshot testing library to make it easier to
/// compare string diffs.
#[cfg(test)]
mod tests {
    use crate::{Circle, Point, Rectangle, draw};

    #[test]
    fn rectangle_simple() {
        let res = draw(
            8,
            8,
            &[&Rectangle::new(Point::from_row_col(3, 4), 4, 2, 'x')],
        );
        insta::assert_snapshot!(res, @r"
        ........
        ........
        ........
        ....xxxx
        ....xxxx
        ........
        ........
        ........
        ");
    }

    #[test]
    fn rectangle_non_square_canvas() {
        let res = draw(
            8,
            12,
            &[&Rectangle::new(Point::from_row_col(3, 4), 4, 2, 'x')],
        );
        insta::assert_snapshot!(res, @r"
        ............
        ............
        ............
        ....xxxx....
        ....xxxx....
        ............
        ............
        ............
        ");
    }

    #[test]
    fn rectangle_oob() {
        let res = draw(
            3,
            2,
            &[&Rectangle::new(Point::from_row_col(0, 1), 5, 2, 'x')],
        );
        insta::assert_snapshot!(res, @r"
        .x
        .x
        ..
        ");
    }

    #[test]
    fn rectangle_other_char() {
        let res = draw(
            4,
            4,
            &[&Rectangle::new(Point::from_row_col(0, 0), 2, 2, 'p')],
        );
        insta::assert_snapshot!(res, @r"
        pp..
        pp..
        ....
        ....
        ");
    }

    #[test]
    fn circle_simple() {
        let res = draw(12, 12, &[&Circle::new(Point::from_row_col(6, 6), 3, 'x')]);
        insta::assert_snapshot!(res, @r"
        ............
        ............
        ............
        ....xxxxx...
        ...xxxxxxx..
        ...xxxxxxx..
        ...xxxxxxx..
        ...xxxxxxx..
        ...xxxxxxx..
        ....xxxxx...
        ............
        ............
        ");
    }

    #[test]
    fn multiple() {
        let res = draw(
            15,
            15,
            &[
                &Circle::new(Point::from_row_col(7, 7), 3, 'x'),
                &Rectangle::new(Point::from_row_col(1, 1), 10, 2, 'o'),
            ],
        );
        insta::assert_snapshot!(res, @r"
        ...............
        .oooooooooo....
        .oooooooooo....
        ...............
        .....xxxxx.....
        ....xxxxxxx....
        ....xxxxxxx....
        ....xxxxxxx....
        ....xxxxxxx....
        ....xxxxxxx....
        .....xxxxx.....
        ...............
        ...............
        ...............
        ...............
        ");
    }

    #[test]
    fn overwrite() {
        let res = draw(
            15,
            15,
            &[
                &Circle::new(Point::from_row_col(7, 7), 3, 'x'),
                &Rectangle::new(Point::from_row_col(1, 1), 7, 14, 'o'),
                &Circle::new(Point::from_row_col(7, 5), 3, '.'),
                &Rectangle::new(Point::from_row_col(5, 3), 5, 5, 'J'),
            ],
        );
        insta::assert_snapshot!(res, @r"
        ...............
        .ooooooo.......
        .ooooooo.......
        .ooooooo.......
        .oo.....xx.....
        .o.JJJJJ.xx....
        .o.JJJJJ.xx....
        .o.JJJJJ.xx....
        .o.JJJJJ.xx....
        .o.JJJJJ.xx....
        .oo.....xx.....
        .ooooooo.......
        .ooooooo.......
        .ooooooo.......
        .ooooooo.......
        ");
    }
}
