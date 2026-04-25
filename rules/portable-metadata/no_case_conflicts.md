---
title: 'no_case_conflicts'
description: 'alint rule kind `no_case_conflicts` (Portable metadata family).'
sidebar:
  order: 1
---

Flag paths that differ only by case (e.g. `README.md` + `readme.md`). They can't coexist on macOS HFS+/APFS or Windows NTFS defaults, so a Linux-only dev committing both breaks checkouts for teammates.

