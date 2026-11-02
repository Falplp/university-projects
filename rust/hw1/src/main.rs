// You can use this file for experiments.
// Exercises with unit tests can be found in the `tests` directory.

pub fn factorial(n: u64) -> u64 {
    (1..=n).product()
}

pub fn find_largest(numbers: [i32; 10]) -> i32 {
    let mut largest = numbers[0];

    for number in numbers {
        if number > largest {
            largest = number;
        }
    }

    largest
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn normalize(self) -> Self {
        let length = self.length();

        if length == 0.0 {
            self
        } else {
            Self::new(self.x / length, self.y / length, self.z / length)
        }
    }
}

pub fn luhn_algorithm(number: u64) -> bool {
    if number < 10 {
        return false;
    }

    let mut remaining = number;
    let mut sum = 0;
    let mut double = false;

    while remaining > 0 {
        let mut digit = remaining % 10;
        remaining /= 10;

        if double {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }

        sum += digit;
        double = !double;
    }

    sum % 10 == 0
}

pub fn match_parentheses(input: &str) -> bool {
    let mut stack = Vec::new();

    for character in input.chars() {
        match character {
            '(' | '[' | '{' => stack.push(character),
            ')' => {
                if stack.pop() != Some('(') {
                    return false;
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return false;
                }
            }
            '}' => {
                if stack.pop() != Some('{') {
                    return false;
                }
            }
            _ => {}
        }
    }

    stack.is_empty()
}

pub fn sanitize(input: &str) -> &str {
    let mut input = input;

    loop {
        let sanitized = input
            .trim_end_matches('x')
            .trim_end_matches('X')
            .trim_end_matches('o')
            .trim_end_matches('O');

        if let Some(stripped) = sanitized
            .strip_suffix(".exe")
            .or_else(|| sanitized.strip_suffix(".EXE"))
            .or_else(|| sanitized.strip_suffix(".Exe"))
            .or_else(|| sanitized.strip_suffix(".eXe"))
            .or_else(|| sanitized.strip_suffix(".exE"))
            .or_else(|| sanitized.strip_suffix(".EXe"))
            .or_else(|| sanitized.strip_suffix(".ExE"))
            .or_else(|| sanitized.strip_suffix(".eXE"))
        {
            input = stripped;
            continue;
        }

        if sanitized == input {
            return input;
        }

        input = sanitized;
    }
}

fn main() {
    println!("Hello Rust!");
}
