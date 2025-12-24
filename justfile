# stable-mir-json justfile

# Use nightly toolchain specified in rust-toolchain.toml
export RUSTUP_TOOLCHAIN := ""

# Default: list available recipes
default:
    @just --list

# Build the project
build:
    cargo build

# Build release
release:
    cargo build --release

# Run tests
test:
    make integration-test

# Format code
fmt:
    cargo fmt

# Lint
lint:
    cargo clippy

# Clean build artifacts
clean:
    cargo clean
    rm -rf output-html output-dot output-d2 output-md

# Test programs directory
test_dir := "tests/integration/programs"

# Generate HTML output for all test programs
html:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-html
    for rust in {{test_dir}}/*.rs; do
        name=$(basename "${rust%.rs}")
        echo "Generating HTML for $name..."
        cargo run -- --html -Zno-codegen --out-dir output-html "$rust" 2>/dev/null || true
        # Move the generated file to have a cleaner name
        if [ -f "output-html/${name}.smir.html" ]; then
            echo "  -> output-html/${name}.smir.html"
        fi
    done
    echo "Done. HTML files in output-html/"

# Generate HTML for a single file
html-file file:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-html
    name=$(basename "{{file}}" .rs)
    cargo run -- --html -Zno-codegen --out-dir output-html "{{file}}"
    echo "Generated: output-html/${name}.smir.html"

# Generate DOT output for all test programs
dot:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-dot
    for rust in {{test_dir}}/*.rs; do
        name=$(basename "${rust%.rs}")
        echo "Generating DOT for $name..."
        cargo run -- --dot -Zno-codegen --out-dir output-dot "$rust" 2>/dev/null || true
    done
    echo "Done. DOT files in output-dot/"

# Generate D2 output for all test programs
d2:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-d2
    for rust in {{test_dir}}/*.rs; do
        name=$(basename "${rust%.rs}")
        echo "Generating D2 for $name..."
        cargo run -- --d2 -Zno-codegen --out-dir output-d2 "$rust" 2>/dev/null || true
    done
    echo "Done. D2 files in output-d2/"

# Generate Markdown output for all test programs
md:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-md
    for rust in {{test_dir}}/*.rs; do
        name=$(basename "${rust%.rs}")
        echo "Generating Markdown for $name..."
        cargo run -- --md -Zno-codegen --out-dir output-md "$rust" 2>/dev/null || true
        if [ -f "output-md/${name}.smir.md" ]; then
            echo "  -> output-md/${name}.smir.md"
        fi
    done
    echo "Done. Markdown files in output-md/"

# Generate Markdown for a single file
md-file file:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-md
    name=$(basename "{{file}}" .rs)
    cargo run -- --md -Zno-codegen --out-dir output-md "{{file}}"
    echo "Generated: output-md/${name}.smir.md"

# Generate all output formats
all: html dot d2 md

# Generate HTML with embedded SVG call graph (collapsible)
html-graph:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-html
    for rust in tests/integration/programs/*.rs; do
        name=$(basename "${rust%.rs}")
        echo "Generating HTML+Graph for $name..."
        cargo run -- --html -Zno-codegen --out-dir output-html "$rust" 2>/dev/null || true
        cargo run -- --dot -Zno-codegen --out-dir output-html "$rust" 2>/dev/null || true
        html_file="output-html/${name}.smir.html"
        dot_file="output-html/${name}.smir.dot"
        if [ -f "$html_file" ] && [ -f "$dot_file" ]; then
            svg_file=$(mktemp)
            # Remove fixed width/height, add id for pan-zoom
            dot -Tsvg "$dot_file" 2>/dev/null | awk '/<svg/,0' | \
                sed 's/width="[^"]*pt" height="[^"]*pt"/id="call-graph"/' > "$svg_file"
            # Escape HTML in source file
            src_file=$(mktemp)
            sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g' "$rust" > "$src_file"
            tmp=$(mktemp)
            awk -v svgfile="$svg_file" -v srcfile="$src_file" -v srcname="$(basename "$rust")" '
                /<\/h1>/ {
                    print
                    print "    <details class=\"source-section\">"
                    print "    <summary>Source: " srcname "</summary>"
                    print "    <pre class=\"source-code\"><code class=\"language-rust\">"
                    while ((getline line < srcfile) > 0) print line
                    close(srcfile)
                    print "    </code></pre>"
                    print "    </details>"
                    print "    <details class=\"graph-section\" open>"
                    print "    <summary>Call Graph</summary>"
                    print "    <div class=\"graph-controls\">"
                    print "        <button onclick=\"panZoom.zoomIn()\">Zoom In</button>"
                    print "        <button onclick=\"panZoom.zoomOut()\">Zoom Out</button>"
                    print "        <button onclick=\"panZoom.resetZoom();panZoom.resetPan()\">Reset</button>"
                    print "        <button onclick=\"panZoom.fit()\">Fit</button>"
                    print "        <button onclick=\"enterFullscreen()\">Fullscreen</button>"
                    print "    </div>"
                    print "    <div class=\"graph-container\">"
                    while ((getline line < svgfile) > 0) print line
                    close(svgfile)
                    print "    </div>"
                    print "    </details>"
                    print "    <div class=\"fullscreen-overlay\" id=\"fs-overlay\">"
                    print "      <div class=\"fs-controls\">"
                    print "        <button onclick=\"fsPanZoom.zoomIn()\">Zoom In</button>"
                    print "        <button onclick=\"fsPanZoom.zoomOut()\">Zoom Out</button>"
                    print "        <button onclick=\"fsPanZoom.resetZoom();fsPanZoom.resetPan()\">Reset</button>"
                    print "        <button onclick=\"fsPanZoom.fit();fsPanZoom.center()\">Fit</button>"
                    print "        <button onclick=\"exitFullscreen()\">Exit</button>"
                    print "      </div>"
                    print "      <div class=\"fs-graph\" id=\"fs-graph\"></div>"
                    print "    </div>"
                    print "    <script>"
                    print "      hljs.highlightAll();"
                    print "      var panZoom = svgPanZoom(\"#call-graph\", {zoomEnabled:true, controlIconsEnabled:false, fit:true, center:true, minZoom:0.1, maxZoom:20});"
                    print "      var fsPanZoom = null;"
                    print "      function enterFullscreen() {"
                    print "        var overlay = document.getElementById(\"fs-overlay\");"
                    print "        var fsGraph = document.getElementById(\"fs-graph\");"
                    print "        var svg = document.getElementById(\"call-graph\").cloneNode(true);"
                    print "        svg.id = \"fs-call-graph\";"
                    print "        fsGraph.innerHTML = \"\";"
                    print "        fsGraph.appendChild(svg);"
                    print "        overlay.requestFullscreen().then(function() {"
                    print "          overlay.classList.add(\"active\");"
                    print "          fsPanZoom = svgPanZoom(\"#fs-call-graph\", {zoomEnabled:true, controlIconsEnabled:false, fit:true, center:true, minZoom:0.1, maxZoom:20});"
                    print "        });"
                    print "      }"
                    print "      function exitFullscreen() {"
                    print "        if (document.fullscreenElement) document.exitFullscreen();"
                    print "      }"
                    print "      document.addEventListener(\"fullscreenchange\", function() {"
                    print "        var overlay = document.getElementById(\"fs-overlay\");"
                    print "        if (!document.fullscreenElement) {"
                    print "          overlay.classList.remove(\"active\");"
                    print "          if (fsPanZoom) { fsPanZoom.destroy(); fsPanZoom = null; }"
                    print "        }"
                    print "      });"
                    print "      document.addEventListener(\"keydown\", function(e) { if (e.key === \"Escape\" && document.fullscreenElement) exitFullscreen(); });"
                    print "    </script>"
                    next
                }
                { print }
            ' "$html_file" > "$tmp" && mv "$tmp" "$html_file"
            rm -f "$dot_file" "$svg_file" "$src_file"
            echo "  -> $html_file (with graph)"
        fi
    done
    # Generate index.html
    echo "Generating index.html..."
    ./scripts/generate-index.sh output-html
    echo "  -> output-html/index.html"
    echo "Done. HTML files with graphs in output-html/"

# Generate HTML+Graph for a single file
html-graph-file file:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-html
    name=$(basename "{{file}}" .rs)
    cargo run -- --html -Zno-codegen --out-dir output-html "{{file}}"
    cargo run -- --dot -Zno-codegen --out-dir output-html "{{file}}"
    html_file="output-html/${name}.smir.html"
    dot_file="output-html/${name}.smir.dot"
    if [ -f "$html_file" ] && [ -f "$dot_file" ]; then
        svg_file=$(mktemp)
        # Remove fixed width/height, add id for pan-zoom
        dot -Tsvg "$dot_file" | awk '/<svg/,0' | \
            sed 's/width="[^"]*pt" height="[^"]*pt"/id="call-graph"/' > "$svg_file"
        # Escape HTML in source file
        src_file=$(mktemp)
        sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g' "{{file}}" > "$src_file"
        tmp=$(mktemp)
        awk -v svgfile="$svg_file" -v srcfile="$src_file" -v srcname="$(basename '{{file}}')" '
            /<\/h1>/ {
                print
                print "    <details class=\"source-section\">"
                print "    <summary>Source: " srcname "</summary>"
                print "    <pre class=\"source-code\"><code class=\"language-rust\">"
                while ((getline line < srcfile) > 0) print line
                close(srcfile)
                print "    </code></pre>"
                print "    </details>"
                print "    <details class=\"graph-section\" open>"
                print "    <summary>Call Graph</summary>"
                print "    <div class=\"graph-controls\">"
                print "        <button onclick=\"panZoom.zoomIn()\">Zoom In</button>"
                print "        <button onclick=\"panZoom.zoomOut()\">Zoom Out</button>"
                print "        <button onclick=\"panZoom.resetZoom();panZoom.resetPan()\">Reset</button>"
                print "        <button onclick=\"panZoom.fit()\">Fit</button>"
                print "        <button onclick=\"enterFullscreen()\">Fullscreen</button>"
                print "    </div>"
                print "    <div class=\"graph-container\">"
                while ((getline line < svgfile) > 0) print line
                close(svgfile)
                print "    </div>"
                print "    </details>"
                print "    <div class=\"fullscreen-overlay\" id=\"fs-overlay\">"
                print "      <div class=\"fs-controls\">"
                print "        <button onclick=\"fsPanZoom.zoomIn()\">Zoom In</button>"
                print "        <button onclick=\"fsPanZoom.zoomOut()\">Zoom Out</button>"
                print "        <button onclick=\"fsPanZoom.resetZoom();fsPanZoom.resetPan()\">Reset</button>"
                print "        <button onclick=\"fsPanZoom.fit();fsPanZoom.center()\">Fit</button>"
                print "        <button onclick=\"exitFullscreen()\">Exit</button>"
                print "      </div>"
                print "      <div class=\"fs-graph\" id=\"fs-graph\"></div>"
                print "    </div>"
                print "    <script>"
                print "      hljs.highlightAll();"
                print "      var panZoom = svgPanZoom(\"#call-graph\", {zoomEnabled:true, controlIconsEnabled:false, fit:true, center:true, minZoom:0.1, maxZoom:20});"
                print "      var fsPanZoom = null;"
                print "      function enterFullscreen() {"
                print "        var overlay = document.getElementById(\"fs-overlay\");"
                print "        var fsGraph = document.getElementById(\"fs-graph\");"
                print "        var svg = document.getElementById(\"call-graph\").cloneNode(true);"
                print "        svg.id = \"fs-call-graph\";"
                print "        fsGraph.innerHTML = \"\";"
                print "        fsGraph.appendChild(svg);"
                print "        overlay.requestFullscreen().then(function() {"
                print "          overlay.classList.add(\"active\");"
                print "          fsPanZoom = svgPanZoom(\"#fs-call-graph\", {zoomEnabled:true, controlIconsEnabled:false, fit:true, center:true, minZoom:0.1, maxZoom:20});"
                print "        });"
                print "      }"
                print "      function exitFullscreen() {"
                print "        if (document.fullscreenElement) document.exitFullscreen();"
                print "      }"
                print "      document.addEventListener(\"fullscreenchange\", function() {"
                print "        var overlay = document.getElementById(\"fs-overlay\");"
                print "        if (!document.fullscreenElement) {"
                print "          overlay.classList.remove(\"active\");"
                print "          if (fsPanZoom) { fsPanZoom.destroy(); fsPanZoom = null; }"
                print "        }"
                print "      });"
                print "      document.addEventListener(\"keydown\", function(e) { if (e.key === \"Escape\" && document.fullscreenElement) exitFullscreen(); });"
                print "    </script>"
                next
            }
            { print }
        ' "$html_file" > "$tmp" && mv "$tmp" "$html_file"
        rm -f "$dot_file" "$svg_file" "$src_file"
        echo "Generated: $html_file (with graph)"
    fi

# List available test programs
list-tests:
    @ls -1 {{test_dir}}/*.rs | xargs -n1 basename | sed 's/\.rs$//'

# === WASM Explorer ===

# Build WASM explorer (dev)
wasm-dev:
    wasm-pack build --dev --target web --out-dir www/pkg mir-explorer

# Build WASM explorer (release)
wasm-release:
    wasm-pack build --release --target web --out-dir www/pkg mir-explorer

# Build and serve WASM explorer locally
wasm-serve: wasm-dev
    python3 -m http.server 8080 -d mir-explorer/www

# Build project with embedded WASM support
wasm-embed-build: wasm-release
    cargo build

# Generate WASM-embedded HTML for all test programs
html-wasm: wasm-embed-build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-html
    for rust in {{test_dir}}/*.rs; do
        name=$(basename "${rust%.rs}")
        echo "Generating WASM explorer for $name..."
        cargo run -- --wasm-explore -Zno-codegen --out-dir output-html "$rust" 2>/dev/null || true
        if [ -f "output-html/${name}.wasm-explore.html" ]; then
            echo "  -> output-html/${name}.wasm-explore.html"
        fi
    done
    # Generate index.html
    echo "Generating index.html..."
    ./scripts/generate-index.sh output-html
    echo "  -> output-html/index.html"
    echo "Done. WASM explorer HTML files in output-html/"

# Generate WASM-embedded HTML for a single file
html-wasm-file file: wasm-embed-build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p output-html
    name=$(basename "{{file}}" .rs)
    cargo run -- --wasm-explore -Zno-codegen --out-dir output-html "{{file}}"
    echo "Generated: output-html/${name}.wasm-explore.html"

# === TUI Explorer ===

# Build TUI explorer
tui-build:
    cargo build -p mir-tui

# Build TUI explorer (release)
tui-release:
    cargo build -p mir-tui --release

# Run TUI on a file (generates JSON then opens TUI)
tui file:
    #!/usr/bin/env bash
    set -euo pipefail
    name=$(basename "{{file}}" .rs)
    cargo run -- --explore-json -Zno-codegen "{{file}}"
    cargo run -p mir-tui -- "${name}.explore.json"

# Run TUI on existing explore.json file
tui-json file:
    cargo run -p mir-tui -- "{{file}}"
