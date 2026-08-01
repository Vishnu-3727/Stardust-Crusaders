# DRAFT — not yet filed

Target: https://github.com/yougov/fuzzy/issues/new
Status: written 2026-08-01, held locally pending review.

---

**Title:** Double Metaphone: the `SC` rules are unreachable — a missing brace
makes `science` code as `SKNK`

---

Related to (but independent of) the soft-C report. `SC` followed by anything
other than `H` never reaches its own handling at all.

```python
>>> import fuzzy
>>> m = fuzzy.DMetaphone()
>>> m('science')
[b'SKNK', None]       # expected [b'SNS', None]
>>> m('scissors')
[b'SKSR', None]       # expected [b'SSRS', None]
>>> m('scene')
[b'SKN', None]        # expected [b'SN', None]
>>> m('scent')
[b'SKNT', None]       # expected [b'SNT', None]
```

`SCH` still works — `school` → `SKL` is correct — because that path is the one
that is live.

The expected values above were checked against the `metaphone` package on PyPI
(`from metaphone import doublemetaphone`), not asserted from the algorithm
description.

## Cause

`src/double_metaphone.c`, in `case 'S':`, around line 935. Reading the braces
rather than the indentation:

```c
if (StringAt(original, current, 2, "SC", ""))
  {                                          /* A */
      /* Schlesinger's rule */
      if (GetAt(original, current + 2) == 'H')
	  {                                    /* B */
	  if (StringAt(original, (current + 3), 2, "OO", "ER", "EN",
	               "UY", "ED", "EM", ""))
	    { ...  current += 3; break; }
	  else
	    { ...  current += 3; break; }

	  /* the brace that should close B belongs HERE */

      if (StringAt(original, (current + 2), 1, "I", "E", "Y", ""))
	{ ...  current += 3; break; }
      /* else */
      MetaphAdd(primary, "SK");
      MetaphAdd(secondary, "SK");
      current += 3;
      break;
  }                                          /* closes B */
}                                            /* closes A */
```

The brace that should close the `GetAt(current + 2) == 'H'` test is missing, so
the `SC`+`I`/`E`/`Y` rule and the default `SK` rule sit **inside** that block —
and both arms of the if/else above them already `break`. They are dead code.

So an `SC` not followed by `H` falls out of block A entirely and lands on the
generic S handling further down, which emits `S` and advances by one. The `C` is
then picked up by the `case 'C':` bug (see the companion report), which emits
`K` and advances by two. `SC` + vowel therefore reliably produces `SK` plus a
dropped letter.

The indentation in the file suggests the author's intent matches the reference
implementation; only the brace is wrong.

## Fix

Close block B after the inner if/else:

```c
      if (GetAt(original, current + 2) == 'H')
	{
	  if (StringAt(original, (current + 3), 2, "OO", "ER", "EN",
	               "UY", "ED", "EM", ""))
	    { ... }
	  else
	    { ... }
	}                                    /* <-- add this */

      if (StringAt(original, (current + 2), 1, "I", "E", "Y", ""))
	{ ... }
```

Same caveat as the companion report: this changes generated codes for any word
containing `SC`, so stored codes will stop matching.

## Note on the `IsVowel(original, 3)` in the same block

```c
if ((current == 0) && !IsVowel(original, 3) && (GetAt(original, 3) != 'W'))
```

Position `3` is hardcoded rather than relative to `current`. Inside
`current == 0` it happens to mean `current + 3`, so it is correct today, but it
will be wrong the moment anyone moves this branch. Worth making relative while
the surrounding code is being touched. Not filing this separately — it is not
currently a defect.

Found while porting this library to Rust for the Port Mortem 2026 hackathon.
Confirmed against `src/double_metaphone.c` compiled unmodified.
