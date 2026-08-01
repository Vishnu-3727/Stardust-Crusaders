#!/usr/bin/env python3
"""NYSIIS reference oracle — the original `nysiis()` from yougov/fuzzy's
`src/fuzzy.pyx`, mechanically extracted.

Why extraction rather than a compiled build: `nysiis` is the one algorithm in
the library that never crosses into C. Its `cdef` locals (`i`, `start`, `stop`,
`suffix`, `first`) are only ever bound to Python objects, so Cython compiles it
to the same semantics CPython gives this file. Building the 2017 extension adds
a toolchain, not fidelity.

The only edits from the .pyx are the removal of `cdef` declarations and the
`cdef extern` blocks, which declare C functions this routine never calls. The
dict literals, the control flow, and the ordering are untouched — diff it
against src/fuzzy.pyx lines 19-185.

Speaks the same line protocol as dm_oracle.exe: one word per line in, one code
per line out.
"""

import re
import sys

_nysiis_suffix_map = {
    'IX': 'IC',
    'EX': 'EC',
    'YE': 'Y',
    'EE': 'Y',
    'IE': 'Y',
    'DT': 'D',
    'RT': 'D',
    'RD': 'D',
    'NT': 'D',
    'ND': 'D'
}

_nysiis_transforms = {
    'AY':  'Y',
    'DG':  'G',
    'E':   'A',
    'EY':  'Y',
    'GHT': 'GT',
    'K':   'C',
    'KN':  'N',
    'I':   'A',
    'IY':  'Y',
    'O':   'A',
    'OY':  'Y',
    'PH':  'F',
    'SH':  'S',
    'SCH': 'S',
    'U':   'A',
    'UY':  'Y',
    'WR':  'R',
    'YW':  'Y'
}

_nysiis_trans_not_first = {
    'AH': 'A',
    'AW': 'A',
    'EH': 'A',
    'EV': 'AF',
    'EW': 'A',
    'HA': 'A',
    'HE': 'A',
    'HI': 'A',
    'HO': 'A',
    'HU': 'A',
    'IH': 'A',
    'IW': 'A',
    'M':  'N',
    'OH': 'A',
    'OW': 'A',
    'Q':  'G',
    'UH': 'A',
    'UW': 'A',
    'Z':  'S'
}

_nysiis_trans_middle = {
    'Y': 'A'
}

_non_AZ = re.compile('[^A-Z]')


def nysiis(s):
    # Normally we would strip out Roman numerals and name suffixes,
    # but we are not going to use this for person names.

    # Strip out anything non-alpha
    s = _non_AZ.sub('', s.upper())
    start, stop = 0, len(s)

    first = ''
    if stop:
        foo = s[0]
        first = foo

    # Find index without trailing SZs
    i = stop
    while i and s[i-1] in 'SZ':
        i = i - 1
    stop = i

    # Initial MAC -> MC, PF -> F
    if s[:3] == 'MAC':
        s = 'MC' + s[3:]
        stop = stop - 1
    elif s[:2] == 'PF':
        start = 1

    # Translate 2-character suffix elements
    suffix = ''
    while (stop - start) > 2:
        x = s[stop-2:stop]

        if x in _nysiis_suffix_map:
            y = _nysiis_suffix_map[x] + suffix
            suffix, stop = y, stop - 2
        else:
            break

    s = s[start:stop] + suffix

    # Build a list of adjacent components while performing transformations
    r = []
    i = start = 0
    stop = len(s)
    while i < stop:
        remain = stop-i  # number of letters including this one

        app = ''

        for l in 3, 2, 1:
            if remain >= l:
                x = s[i:i+l]
                if x in _nysiis_transforms:
                    app = _nysiis_transforms[x]
                    break

                elif i > start:
                    if x in _nysiis_trans_not_first:
                        app = _nysiis_trans_not_first[x]
                        break

                    elif i < (stop-1) and x in _nysiis_trans_middle:
                        app = _nysiis_trans_middle[x]
                        break

        if app:
            r.extend(app)
            i = i + l
        else:
            r.append(s[i])
            i = i + 1

    # Remove trailing vowels
    stop = len(r)
    while stop and r[stop-1] in 'AEIOU':
        stop = stop - 1

    # If first char of original string is a A vowel, use it
    if first in 'AEIOU':
        if r:
            r[0] = first
        else:
            r = [first]

    # Filter out repeated characters
    q, last = [], ''
    for x in r[:stop]:
        if x == last:
            continue

        q.append(x)
        last = x

    return ''.join(q)


def main():
    # NYSIIS takes str, so the protocol has to be UTF-8 on both ends. Without
    # this, Windows hands us cp1252 and every non-Latin-1 input silently
    # becomes a different word than the port was asked about.
    sys.stdin.reconfigure(encoding='utf-8')
    sys.stdout.reconfigure(encoding='utf-8')
    for line in sys.stdin:
        print(nysiis(line.rstrip('\n').rstrip('\r')), flush=True)


if __name__ == '__main__':
    main()
