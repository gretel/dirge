/*
 * Test program for DAP integration tests — C variant.
 *
 * Exercises DAP variable inspection across every DAP type:
 *   - Scalars:   int, long, float, double, char, _Bool
 *   - Strings:   const char*, char[]
 *   - Arrays:    int[], double[]
 *   - Structs:   simple (Counter), with nested members (AdapterInfo)
 *   - Nested:    struct-of-structs (Connection containing Counter + AdapterInfo)
 *   - Pointers:  int*, struct*, nested pointers
 *   - Enums:     error_kind + error_code
 *   - Unions:    variant payload (value_or_message)
 *   - Volatile:  volatile int (tests that debugger handles volatile correctly)
 *   - Typedefs:  counter_t alias
 *
 * Intended to be run with lldb-dap or gdb.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ── scalars, strings, arrays ─────────────────────────────────────── */

static int number = 42;
static long big_number = 2147483648L;
static float ratio = 3.14f;
static double pi = 3.141592653589793;
static char letter = 'X';
static int flag = 1;                    /* _Bool / int */
static const char *greeting = "Hello, DAP!";
static char buffer[64] = "initialized buffer with trailing garbage";

static int items[] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 20};
static size_t item_count = sizeof(items) / sizeof(items[0]);

static double coords[] = {1.1, 2.2, 3.3, 4.4, 5.5};
static size_t coord_count = sizeof(coords) / sizeof(coords[0]);

/* ── enum + union ──────────────────────────────────────────────────── */

typedef enum {
    ERR_NONE = 0,
    ERR_TIMEOUT,
    ERR_DISCONNECTED,
    ERR_INVALID,
} error_kind;

typedef union {
    long   numeric;
    char   message[64];
} payload;

/* ── structs + typedef ─────────────────────────────────────────────── */

typedef struct {
    int    value;
    double threshold;
    const char *label;
} Counter;

typedef struct {
    const char   *name;
    int           major;
    int           minor;
    int           patch;
    int           connected;           /* bool-like */
} AdapterInfo;

typedef struct {
    Counter         counter;
    AdapterInfo     adapter;
    int             port;
    error_kind      last_error;
    payload         last_payload;
} Connection;

/* ── recursive function (deep stack trace) ──────────────────────────── */

long factorial(long n) {
    if (n <= 1) return 1;
    return n * factorial(n - 1);
}

/* ── loop with conditional (conditional bp target) ─────────────────── */

void process_items(const int *src, size_t count, int *dst) {
    for (size_t i = 0; i < count; i++) {
        int doubled = src[i] * 2;       /* conditional bp: src[i] > 10 */
        dst[i] = doubled;
    }
}

/* ── nested calls (step-in / step-out target) ─────────────────────── */

int inner(int x) {
    int square = x * x;
    return square;
}

int middle(int x) {
    int y = x + 3;
    int z = inner(y);
    return z + 1;
}

int outer(void) {
    int result = middle(5);
    return result * 2;
}

/* ── volatile test ──────────────────────────────────────────────────── */

static volatile int heartbeat = 0;

/* ── main ──────────────────────────────────────────────────────────── */

int main(void) {
    int doubled[20] = {0};
    volatile int local_volatile = 99;

    /* ── complex struct assembly ──────────────────────────────────── */
    Counter c = {.value = 10, .threshold = 0.5, .label = "main-counter"};

    AdapterInfo info = {
        .name      = "debugpy",
        .major     = 1,
        .minor     = 8,
        .patch     = 13,
        .connected = 1,
    };

    Connection conn = {
        .counter     = c,
        .adapter     = info,
        .port        = 5678,
        .last_error  = ERR_TIMEOUT,
        .last_payload.message = "connection attempt timed out after 30 seconds",
    };

    heartbeat = 1;  /* [bp-1a] after volatile write — inspect heartbeat */

    int *heap_number = malloc(sizeof(int));
    if (heap_number) *heap_number = 999;
    int *heap_array = malloc(sizeof(int) * 4);
    if (heap_array) {
        heap_array[0] = 10;
        heap_array[1] = 20;
        heap_array[2] = 30;
        heap_array[3] = 40;
    }

    /* [bp-1] inspect: number, greeting, items[3], c.value, c.label,
     *   info.name, info.major, conn.port, conn.adapter.name,
     *   conn.last_error, conn.last_payload.message,
     *   *heap_number, heap_array[2], heartbeat, local_volatile */

    printf("number = %d\n", number);
    printf("pi    = %.15f\n", pi);
    printf("text  = %s\n", greeting);
    printf("flag  = %d\n", flag);
    printf("info  = %s v%d.%d.%d\n", info.name, info.major, info.minor, info.patch);
    printf("conn.port = %d\n", conn.port);

    /* loop: step_over friendly */
    process_items(items, item_count, doubled);
    printf("doubled[0] = %d, doubled[last] = %d\n",
           doubled[0], doubled[item_count - 1]);

    /* [bp-2] after loop: inspect doubled[3], coords[2], strerror, etc. */

    /* recursion */
    long fact = factorial(5);
    printf("factorial(5) = %ld\n", fact);

    /* object mutation */
    c.value += 1;
    c.value += 1;
    printf("counter.value = %d\n", c.value);

    /* [bp-3] after counter ops: inspect c.value,  c.threshold */

    /* nested calls */
    int outer_result = outer();
    printf("outer_result = %d\n", outer_result);

    /* [bp-4] near end: inspect outer_result, heap_number */

    int x = 10;
    int y = 20;
    int z = x + y;
    printf("z = %d\n", z);

    free(heap_number);
    free(heap_array);
    return 0;
}
