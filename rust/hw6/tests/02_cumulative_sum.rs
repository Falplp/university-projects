//! Run this file with `cargo test --test 02_cumulative_sum`.

//! TODO: Implement a struct called `CumulativeSum`, which will be generic over two types - a value
//! type and an iterator over these value types. `CumulativeSum` will itself serve as an iterator,
//! which will return a cumulative sum of the items from the input iterator.
//! E.g. `CumulativeSum::new(vec![1, 2, 3])` will iterate `1`, `3`, and `6`.
//!
//! The iterator has to be **lazy**! It should not copy the whole input array
//! (in other words, it should have space complexity O(1)).
//!
//! Think about the various trait bounds that you will require for `CumulativeSum` to work.
//! What operations have to be supported by the two generic types?

struct CumulativeSum<T, I> {
    iter: I,
    sum: Option<T>,
}

impl<T, I: Iterator<Item = T>> CumulativeSum<T, I> {
    fn new(iter: I) -> Self {
        Self { iter, sum: None }
    }
}

impl<T, I> Iterator for CumulativeSum<T, I>
where
    T: Clone + std::ops::Add<Output = T>,
    I: Iterator<Item = T>,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.iter.next()?;
        let sum = match self.sum.take() {
            Some(previous) => previous + value,
            None => value,
        };
        self.sum = Some(sum.clone());
        Some(sum)
    }
}

/// Below you can find a set of unit tests.
#[cfg(test)]
mod tests {
    use crate::CumulativeSum;
    use std::ops::Add;

    #[test]
    fn empty() {
        assert_eq!(CumulativeSum::new(Vec::<u32>::new().into_iter()).count(), 0);
    }

    #[test]
    fn single() {
        assert_eq!(
            CumulativeSum::new(vec![1].into_iter()).collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn simple_u32() {
        let result = CumulativeSum::new(vec![1, 2, 3].into_iter()).collect::<Vec<_>>();
        assert_eq!(result, vec![1, 3, 6]);
    }

    #[test]
    fn simple_array() {
        let result = CumulativeSum::new([1, 2, 3].into_iter()).collect::<Vec<_>>();
        assert_eq!(result, vec![1, 3, 6]);
    }

    #[test]
    fn simple_lazy_iter() {
        let result = CumulativeSum::new(std::iter::from_fn(|| Some(1)).take(3)).collect::<Vec<_>>();
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[test]
    fn simple_vec() {
        #[derive(Default, Copy, Clone, PartialEq, Debug)]
        struct Vec2D {
            x: u32,
            y: u32,
        }

        impl Add for Vec2D {
            type Output = Self;

            fn add(self, rhs: Self) -> Self::Output {
                Self {
                    x: self.x + rhs.x,
                    y: self.y + rhs.y,
                }
            }
        }

        let result = CumulativeSum::new(
            vec![
                Vec2D { x: 2, y: 6 },
                Vec2D { x: 4, y: 0 },
                Vec2D { x: 2, y: 2 },
                Vec2D { x: 5, y: 3 },
            ]
            .into_iter(),
        )
        .collect::<Vec<_>>();
        assert_eq!(
            result,
            vec![
                Vec2D { x: 2, y: 6 },
                Vec2D { x: 6, y: 6 },
                Vec2D { x: 8, y: 8 },
                Vec2D { x: 13, y: 11 }
            ]
        );
    }
    #[test]
    fn consumes_only_requested_items() {
        let calls = std::cell::Cell::new(0);
        let input = std::iter::from_fn(|| {
            calls.set(calls.get() + 1);
            Some(2)
        });
        let mut sums = CumulativeSum::new(input);
        assert_eq!(calls.get(), 0);
        assert_eq!(sums.next(), Some(2));
        assert_eq!(sums.next(), Some(4));
        assert_eq!(calls.get(), 2);
    }
}
