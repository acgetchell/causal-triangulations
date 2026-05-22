#!/usr/bin/env bash

# ok: causal-triangulations.docs.check-before-fix-command-order
just check
just fix

# ruleid: causal-triangulations.docs.check-before-fix-command-order
just fix
just check

# ok: causal-triangulations.docs.check-before-fix-command-order
just python-check
just python-fix

# ruleid: causal-triangulations.docs.check-before-fix-command-order
just python-fix
just python-check

# ruleid: causal-triangulations.docs.check-before-fix-command-order
just markdown-fix

# Comment-only lines may separate the two commands.
just markdown-check
