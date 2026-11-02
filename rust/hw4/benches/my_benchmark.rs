use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::collections::HashSet;
use std::hint::black_box;

#[derive(Copy, Clone)]
struct Point {
    row: u32,
    col: u32,
}

struct Circle {
    center: Point,
    radius: u32,
    pixel: char,
}

impl Circle {
    fn pixel_at(&self, point: Point) -> Option<char> {
        let dx = point.col.abs_diff(self.center.col);
        let dy = point.row.abs_diff(self.center.row);
        let r = (self.radius as f32) + 0.7;
        if (dx * dx + dy * dy) as f32 <= r * r {
            Some(self.pixel)
        } else {
            None
        }
    }
    fn pixel_at_2(&self, point: Point) -> Option<char> {
        let a = (self.center.row as i32 - point.row as i32).pow(2);
        let b = (self.center.col as i32 - point.col as i32).pow(2);
        let distance = (a + b).isqrt();

        if distance <= self.radius as i32 {
            Some(self.pixel)
        } else {
            None
        }
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    let circle = Circle {
        center: Point { row: 1, col: 2 },
        radius: 5,
        pixel: 'x',
    };
    let point = Point { row: 2, col: 1 };

    c.bench_function("A", |b| {
        b.iter(|| black_box(circle.pixel_at(black_box(point))));
    });
    c.bench_function("B", |b| {
        b.iter(|| black_box(circle.pixel_at_2(black_box(point))));
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
