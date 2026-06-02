// Test program for DAP integration tests — Rust variant.
//
// Exercises DAP variable inspection across Rust-specific types:
//   - Scalars: i32, i64, f32, f64, bool, char, &str, String
//   - Collections: Vec<T>, HashMap<K,V>, [T; N], slices
//   - Structs: named fields, tuple structs, generic structs
//   - Enums: Option<T>, Result<T,E>, custom enum with data
//   - Smart pointers: Box<T>, Rc<T>, raw *const T
//   - Traits: Debug-printable objects
//   - Lifetimes: &'static str, borrowed references

use std::collections::HashMap;
use std::rc::Rc;

// ── structs ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct Counter {
    value: i32,
    threshold: f64,
    label: String,
}

impl Counter {
    fn new(start: i32, label: &str) -> Self {
        Counter { value: start, threshold: 0.5, label: label.to_string() }
    }
    fn increment(&mut self) -> i32 { self.value += 1; self.value }
}

#[derive(Debug)]
struct AdapterInfo {
    name: String,
    version: (u32, u32, u32),          // tuple field
    connected: bool,
}

// ── enums ─────────────────────────────────────────────────────────────

#[derive(Debug)]
enum ErrorKind {
    None,
    Timeout(u32),                        // variant with data
    Disconnected { reason: String },     // variant with named fields
    Invalid,
}

// ── custom trait object ──────────────────────────────────────────────

trait Describable {
    fn describe(&self) -> String;
}

impl Describable for Counter {
    fn describe(&self) -> String {
        format!("Counter(value={}, threshold={})", self.value, self.threshold)
    }
}

// ── functions ────────────────────────────────────────────────────────

fn factorial(n: u64) -> u64 {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

fn process_items(items: &[i32]) -> Vec<i32> {
    items.iter().map(|&item| item * 2).collect()
}

fn inner(x: i32) -> i32 { x * x }
fn middle(x: i32) -> i32 { let y = x + 3; let z = inner(y); z + 1 }
fn outer() -> i32 { let result = middle(5); result * 2 }

// ── main ─────────────────────────────────────────────────────────────

fn main() {
    // scalars
    let text = "Hello, DAP!";            // &str
    let owned = String::from("owned");  // String
    let number: i32 = 42;
    let big: i64 = 9_223_372_036_854_775_807;
    let pi: f64 = 3.141592653589793;
    let flag = true;
    let ch = '🦀';

    // collections
    let items = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 20];
    let mut mapping = HashMap::new();
    mapping.insert("key_a", 100);
    mapping.insert("key_b", 200_i32);
    let fixed: [i32; 5] = [10, 20, 30, 40, 50];

    // structs + enums
    let mut counter = Counter::new(10, "main-counter");
    let adapter = AdapterInfo {
        name: "debugpy".into(),
        version: (1, 8, 13),
        connected: true,
    };
    let last_error = ErrorKind::Timeout(30);
    let desc: &dyn Describable = &counter;

    // smart pointers
    let heap_int = Box::new(999);
    let raw_ptr: *const i32 = heap_int.as_ref() as *const i32;  // lint: allow
    let _ = raw_ptr;
    let shared = Rc::new(42);

    let mut option_val = Some("present");
    let result_val: Result<i32, &str> = Ok(100);

    // [bp-1] inspect: text, number, pi, ch, items.len(), mapping["key_a"],
    //   counter.value, adapter.name, adapter.version.0, last_error,
    //   *heap_int, shared, option_val, result_val, Rc::strong_count(&shared)

    println!("text   = {text}");
    println!("number = {number}");
    println!("pi     = {pi}");
    println!("flag   = {flag}");
    println!("items  = {}", items.len());

    let doubled = process_items(&items);
    println!("doubled[0] = {}, size = {}", doubled[0], doubled.len());

    println!("desc   = {}", desc.describe());

    // Move the mutable ops below the immutable borrow's last use.
    counter.increment();
    counter.increment();
    println!("counter = {}", counter.value);

    // [bp-3] after counter ops

    let outer_result = outer();
    println!("outer = {outer_result}");

    // [bp-4] near end

    let x = 10i32;
    let y = 20i32;
    let z = x + y;
    println!("z = {z}");

    option_val = None;
    println!("option = {option_val:?}");
}
