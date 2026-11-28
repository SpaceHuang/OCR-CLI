#!/bin/sh
# Unix wrapper (plan D-9): forwards the untouched argv to the scenario-driven
# fake binary. The scenario travels via OCRX_FAKE_SCENARIO (env), never argv.
exec node "$(dirname "$0")/../fake-ocrx.mjs" "$@"
