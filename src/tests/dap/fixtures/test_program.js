/**
 * Test program for DAP integration tests — JavaScript variant.
 *
 * Exercises: launch with stopOnEntry, line breakpoints, continue,
 * step over/into, stack trace, variable inspection, expression
 * evaluation.  Intended to be run with the bundled dap_node_adapter.js
 * or any JavaScript DAP adapter.
 */

// ── Counter class for object inspection ──────────────────────────
class Counter {
    constructor(start = 0) {
        this.value = start;
        this.label = "counter";
    }

    increment() {
        this.value++;
        return this.value;
    }
}

// ── recursive function for deeper stack traces ────────────────────
function factorial(n) {
    if (n <= 1) return 1;
    return n * factorial(n - 1);
}

// ── loop with conditional — exercise conditional breakpoints ──────
function processItems(items) {
    const results = [];
    for (const item of items) {
        const doubled = item * 2;  // conditional bp: item > 10
        results.push(doubled);
    }
    return results;
}

// ── nested calls for step_in / step_out ───────────────────────────
function inner(x) {
    const square = x * x;
    return square;
}

function middle(x) {
    const y = x + 3;
    const z = inner(y);
    return z + 1;
}

function outer() {
    const result = middle(5);
    return result * 2;
}

// ── main ───────────────────────────────────────────────────────────
function main() {
    // basic types to inspect
    const text    = "Hello, DAP!";
    const number  = 42;
    const pi      = 3.14159;
    const flag    = true;

    const items   = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 20];
    const mapping = { key_a: 100, key_b: 200 };
    const counter = new Counter(10);

    // [bp-1] inspect locals
    console.log(`text   = ${text}`);
    console.log(`number = ${number}`);
    console.log(`pi     = ${pi}`);
    console.log(`flag   = ${flag}`);

    // loop
    const doubled = processItems(items);
    console.log(`doubled[0] = ${doubled[0]}, size = ${doubled.length}`);

    // [bp-2] after loop

    // recursion
    const fact = factorial(5);
    console.log(`factorial(5) = ${fact}`);

    // object mutation
    counter.increment();
    counter.increment();
    console.log(`counter.value = ${counter.value}`);

    // [bp-3] after counter ops

    // nested calls
    const outerResult = outer();
    console.log(`outer = ${outerResult}`);

    // [bp-4] near end

    const x = 10;
    const y = 20;
    const z = x + y;
    console.log(`z = ${z}`);
}

main();
