/*
 * Test program for DAP integration tests — C++ variant.
 *
 * Exercises DAP variable inspection across C++-specific types:
 *   - std::string, std::vector<T>, std::map<K,V>
 *   - Classes with public/private members, const methods
 *   - Inheritance: Base → Derived with virtual method
 *   - Smart pointers: std::unique_ptr, raw owning pointer
 *   - Templates: std::pair<K,V>, std::array<int,N>
 *   - References: const T& parameters
 *
 * Intended to be run with lldb-dap or gdb.
 */

#include <iostream>
#include <string>
#include <vector>
#include <map>
#include <array>
#include <memory>
#include <numeric>

/* ── inheritance hierarchy ────────────────────────────────────────── */

class Debuggable {
public:
    Debuggable(const char *tag) : tag_(tag) {}
    virtual const char* describe() const { return tag_; }
    virtual ~Debuggable() = default;
private:
    const char *tag_;
};

class Counter : public Debuggable {
public:
    Counter(int start = 0)
        : Debuggable("counter"), value_(start), threshold_(0.5) {}

    int  increment()       { return ++value_; }
    int  value()     const { return value_; }
    double threshold() const { return threshold_; }

    const char* describe() const override { return "Counter::describe"; }

private:
    int    value_;
    double threshold_;
};

/* ── templates + smart pointers ───────────────────────────────────── */

std::vector<int> process_items(const std::vector<int> &items) {
    std::vector<int> results;
    for (auto item : items) {
        int doubled = item * 2;     /* conditional bp: item > 10 */
        results.push_back(doubled);
    }
    return results;
}

long factorial(long n) {
    if (n <= 1) return 1;
    return n * factorial(n - 1);
}

/* nested calls */
int inner(int x) {
    int square = x * x;
    return square;
}
int middle(int x) {
    int y = x + 3;
    int z = inner(y);
    return z + 1;
}
int outer() {
    int result = middle(5);
    return result * 2;
}

/* ── main ────────────────────────────────────────────────────────── */

int main() {
    /* scalars + strings */
    std::string text = "Hello, DAP!";
    int number = 42;
    double pi = 3.141592653589793;
    bool flag = true;

    /* containers */
    std::vector<int> items = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 20};
    std::map<std::string, int> mapping = {{"key_a", 100}, {"key_b", 200}};
    std::array<int, 5> fixed = {10, 20, 30, 40, 50};

    /* objects */
    Counter counter(10);
    std::unique_ptr<int> heap_int = std::make_unique<int>(999);
    int *raw_ptr = heap_int.get();

    /* [bp-1] inspect: text, number, pi, flag, items, mapping,
     *   counter.value(), counter.threshold(), *heap_int, raw_ptr */

    std::cout << "text   = " << text   << std::endl;
    std::cout << "number = " << number << std::endl;
    std::cout << "pi     = " << pi     << std::endl;
    std::cout << "flag   = " << (flag ? "true" : "false") << std::endl;

    auto doubled = process_items(items);
    std::cout << "doubled[0] = " << doubled[0]
              << "  size = "      << doubled.size() << std::endl;

    /* [bp-2] after loop */

    long fact = factorial(5);
    std::cout << "factorial(5) = " << fact << std::endl;

    counter.increment();
    counter.increment();
    std::cout << "counter = " << counter.value() << std::endl;

    /* [bp-3] after counter ops */

    int outer_result = outer();
    std::cout << "outer = " << outer_result << std::endl;

    /* [bp-4] near end */

    int x = 10, y = 20;
    int z = x + y;
    std::cout << "z = " << z << std::endl;

    return 0;
}
