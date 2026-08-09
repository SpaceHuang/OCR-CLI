#!/bin/sh
set -eu

printf '%s\n' 'ocrx macOS environment snapshot'
printf 'date=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
printf 'os=%s\n' "$(sw_vers -productVersion)"
printf 'kernel=%s\n' "$(uname -sr)"
printf 'arch=%s\n' "$(uname -m)"
printf 'rustc=%s\n' "$(rustc --version)"
printf 'cargo=%s\n' "$(cargo --version)"
printf 'cmake=%s\n' "$(cmake --version | sed -n '1p')"

if command -v system_profiler >/dev/null 2>&1; then
    system_profiler SPHardwareDataType SPDisplaysDataType 2>/dev/null \
        | sed -n -e '/Model Identifier:/p' -e '/Chip:/p' -e '/Memory:/p' \
            -e '/Metal:/p' -e '/Total Number of Cores:/p'
fi

if command -v xcodebuild >/dev/null 2>&1; then
    printf 'xcode=%s\n' "$(xcodebuild -version | sed -n '1p')"
fi

if command -v xcrun >/dev/null 2>&1; then
    if xcrun --find metal >/dev/null 2>&1; then
        printf 'metal_lookup=available\n'
    else
        printf 'metal_lookup=unavailable-default-mapping\n'
    fi
fi
