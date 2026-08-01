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
#include "double_metaphone.h"

int main(void) {
    char line[4096];
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
