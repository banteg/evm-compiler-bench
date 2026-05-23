set shell := ["zsh", "-cu"]

build-report-ui:
    npm --prefix report-ui ci
    npm --prefix report-ui run build

prepare-publish channel="dev":
    node scripts/publish-results.mjs --channel={{channel}}

publish-results channel="dev":
    node scripts/publish-results.mjs --upload --channel={{channel}}

publish-dev-results:
    node scripts/publish-results.mjs --upload --channel=dev

publish-prod-results:
    #!/usr/bin/env zsh
    set -euo pipefail
    if [[ "$(git branch --show-current)" != "master" ]]; then
      echo "prod publish must run from master" >&2
      exit 1
    fi
    git diff --quiet
    git diff --cached --quiet
    node scripts/publish-results.mjs --upload --channel=prod

deploy-site:
    npm --prefix report-ui ci
    npm --prefix report-ui run build
    wrangler deploy --env=""

deploy-dev-site:
    npm --prefix report-ui ci
    npm --prefix report-ui run build
    wrangler deploy --env dev

# Create a shareable source + reports archive without local toolchain/build caches.
zip out="":
    #!/usr/bin/env zsh
    set -euo pipefail

    root="${PWD:A}"
    repo="${root:t}"
    sha="$(git rev-parse --short HEAD)"
    stamp="$(date +%Y%m%d)"
    archive="{{out}}"
    if [[ -z "$archive" ]]; then
      archive="${root:h}/${repo}-code-and-reports-${sha}-${stamp}.zip"
    fi

    rm -f "$archive"
    (
      cd "$root:h"
      zip -rq "$archive" "$repo" \
        -x "$repo/.git/*" \
        -x "$repo/.cache/*" \
        -x "$repo/report-ui/node_modules/*" \
        -x "$repo/report-ui/dist/*" \
        -x "$repo/report-ui/.vite/*" \
        -x "$repo/target/debug/*" \
        -x "$repo/target/release/*" \
        -x "$repo/target/.rustc_info.json" \
        -x "$repo/target/CACHEDIR.TAG" \
        -x "$repo/foundry/cache/*" \
        -x "$repo/foundry/out/*" \
        -x "$repo/**/*.DS_Store" \
        -x "$repo/.DS_Store" \
        -x "$repo/*.zip"
    )

    ls -lh "$archive"

# Create a compact frontend + report-model archive for design tools.
zip-design out="":
    #!/usr/bin/env zsh
    set -euo pipefail

    root="${PWD:A}"
    repo="${root:t}"
    sha="$(git rev-parse --short HEAD)"
    stamp="$(date +%Y%m%d)"
    archive="{{out}}"
    if [[ -z "$archive" ]]; then
      archive="${root:h}/${repo}-design-kit-${sha}-${stamp}.zip"
    fi

    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    kit="${repo}-design-kit-${sha}"
    mkdir -p "$tmp/$kit/report-ui" "$tmp/$kit/results/normalized"

    cp "$root/report-ui/index.html" "$tmp/$kit/report-ui/"
    cp "$root/report-ui/package.json" "$tmp/$kit/report-ui/"
    cp "$root/report-ui/package-lock.json" "$tmp/$kit/report-ui/"
    cp "$root/report-ui/vite.config.js" "$tmp/$kit/report-ui/"
    cp -R "$root/report-ui/src" "$tmp/$kit/report-ui/"
    node "$root/report-ui/scripts/make-design-model.mjs" \
      "$root/results/normalized/report-model.json" \
      "$tmp/$kit/results/normalized/report-model.json"
    cp "$root/results/normalized/run-manifest.json" "$tmp/$kit/results/normalized/"

    printf '%s\n' \
      '# EVM Compiler Bench Design Kit' \
      '' \
      'This archive contains only the report frontend source plus enough data to render it.' \
      'The included report-model.json is pruned for design-tool upload limits and is not the full evidence model.' \
      '' \
      'Run:' \
      '' \
      '```sh' \
      'cd report-ui' \
      'npm install' \
      'npm run dev' \
      '```' \
      '' \
      'The dev server reads `../results/normalized/report-model.json`.' \
      > "$tmp/$kit/README.md"

    rm -f "$archive"
    (
      cd "$tmp"
      zip -rq "$archive" "$kit"
    )

    entries="$(zipinfo -1 "$archive" | wc -l | tr -d ' ')"
    ls -lh "$archive"
    echo "entries: $entries"
