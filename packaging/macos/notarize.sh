#!/usr/bin/env bash
set -euo pipefail

artifact=$1
submission=$artifact
if [[ -d "$artifact" ]]; then
  submission="$RUNNER_TEMP/$(basename "$artifact").zip"
  ditto -c -k --keepParent "$artifact" "$submission"
fi

credentials=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER")

result=$(xcrun notarytool submit "$submission" "${credentials[@]}" --wait --timeout 30m --output-format json)
echo "$result"

if [[ "$(jq -r .status <<<"$result")" != Accepted ]]; then
  xcrun notarytool log "$(jq -r .id <<<"$result")" "${credentials[@]}"
  exit 1
fi

xcrun stapler staple "$artifact"
xcrun stapler validate "$artifact"
