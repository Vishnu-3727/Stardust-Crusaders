/* Oracle driver: runs the ORIGINAL yougov/fuzzy C double_metaphone.c unmodified.
 * Reads one word per line on stdin, writes "PRIMARY\tSECONDARY" per line.
 * SECONDARY is emitted as the literal 4 bytes "NULL" when the Cython layer
 * would have returned None (i.e. secondary == primary, or empty string).
 * Mirrors fuzzy.pyx DMetaphone.__call__ exactly:
 *     if o1 == o2: o2 = None
 *     return [o1 and o1[:size] or None, o2 and o2[:size] or None]
 * with size defaulting to 99999 (no truncation beyond the C's own 4-char cut).
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "double_metaphone.h"

/* `--bench`: load the whole corpus first, then time the algorithm alone — the
 * same shape as `fuzzy --bench`, so the two numbers are comparable. Without
 * this the C is measured with per-line IO in the timed region and the Rust is
 * not. Prints "count nanos sink". */
static int bench(void) {
    static char *words[400000];
    char line[4096];
    size_t n = 0;

    while (n < sizeof words / sizeof *words && fgets(line, sizeof line, stdin)) {
        size_t len = strlen(line);
        while (len && (line[len - 1] == '\n' || line[len - 1] == '\r')) line[--len] = '\0';
        words[n] = (char *) malloc(len + 1);
        memcpy(words[n], line, len + 1);
        n++;
    }

    char **codes = (char **) malloc(sizeof(char *) * 2);
    size_t sink = 0;
    clock_t start = clock();
    for (size_t i = 0; i < n; i++) {
        codes[0] = NULL;
        codes[1] = NULL;
        DoubleMetaphone(words[i], codes);
        if (codes[0]) { sink += strlen(codes[0]); free(codes[0]); }
        if (codes[1]) { sink += strlen(codes[1]); free(codes[1]); }
    }
    double seconds = (double) (clock() - start) / CLOCKS_PER_SEC;

    /* sink is printed so the loop cannot be optimised away. */
    printf("%zu %.0f %zu\n", n, seconds * 1e9, sink);
    return 0;
}

int main(int argc, char **argv) {
    char line[4096];

    if (argc > 1 && strcmp(argv[1], "--bench") == 0)
        return bench();

    while (fgets(line, sizeof line, stdin)) {
        size_t n = strlen(line);
        while (n && (line[n - 1] == '\n' || line[n - 1] == '\r')) line[--n] = '\0';

        char **codes = (char **) malloc(sizeof(char *) * 2);
        codes[0] = NULL;
        codes[1] = NULL;
        DoubleMetaphone(line, codes);

        const char *p = codes[0] ? codes[0] : "";
        const char *s = codes[1] ? codes[1] : "";
        int same = (strcmp(p, s) == 0);

        printf("%s\t%s\n", *p ? p : "NULL", (same || !*s) ? "NULL" : s);
        fflush(stdout);
    }
    return 0;
}
