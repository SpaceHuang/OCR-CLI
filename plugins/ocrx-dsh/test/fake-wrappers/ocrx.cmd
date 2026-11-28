@echo off
rem Windows wrapper (plan D-9): the plugin spawns a REAL executable; this
rem .cmd forwards the untouched argv to the scenario-driven fake binary.
rem The scenario travels via OCRX_FAKE_SCENARIO (env), never argv.
node "%~dp0..\fake-ocrx.mjs" %*
