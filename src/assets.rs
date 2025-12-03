//! HTML, CSS, and JavaScript assets for MIR visualization
//!
//! This module centralizes all embedded web assets used by the HTML and explorer outputs,
//! making them easier to maintain and modify.

// =============================================================================
// Shared Assets
// =============================================================================

/// Shared JS helper for rendering locals - used by both standalone and embedded explorers
pub const RENDER_LOCAL_JS: &str = r##"
function renderLocalHtml(local, escapeHtml) {
    const srcName = local.source_name
        ? ` <span class="annotation">(${escapeHtml(local.source_name)})</span>`
        : '';
    const assigns = local.assignments.length > 0
        ? local.assignments.map(a => `bb${a.block_id}: ${escapeHtml(a.value)}`).join(', ')
        : '(arg/ret)';
    return `<li><span class="mir">${escapeHtml(local.name)}: ${escapeHtml(local.ty)}</span>${srcName}
           <br><span class="annotation" style="margin-left:1em">${assigns}</span></li>`;
}
"##;

// =============================================================================
// Standalone Explorer Assets (--explore flag)
// =============================================================================

/// Assets for the standalone interactive MIR explorer
pub mod explorer {
    /// CSS for the standalone explorer
    pub const CSS: &str = r##"
:root {
    --bg: #1a1a2e;
    --bg-panel: #16213e;
    --bg-block: #0f0f1a;
    --text: #eee;
    --text-dim: #888;
    --accent: #8be9fd;
    --green: #50fa7b;
    --purple: #bd93f9;
    --pink: #ff79c6;
    --orange: #ffb86c;
    --border: #333;
}

* { box-sizing: border-box; margin: 0; padding: 0; }

body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg);
    color: var(--text);
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

#header {
    padding: 0.75rem 1.5rem;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 1rem;
}

#header h1 {
    color: var(--accent);
    font-size: 1.25rem;
    margin: 0;
}

#fn-selector select {
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--border);
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
}

#path-bar {
    padding: 0.5rem 1.5rem;
    background: var(--bg-block);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-family: monospace;
    font-size: 0.9rem;
}

.path-label { color: var(--text-dim); }

#path-breadcrumb {
    flex: 1;
    color: var(--green);
}

#path-breadcrumb .current {
    color: var(--pink);
    font-weight: bold;
}

#path-breadcrumb .visited {
    color: var(--text-dim);
}

#reset-btn, #back-btn {
    background: var(--bg-panel);
    color: var(--text);
    border: 1px solid var(--border);
    padding: 0.3rem 0.8rem;
    border-radius: 4px;
    cursor: pointer;
}

#reset-btn:hover, #back-btn:hover {
    background: var(--border);
}

#main {
    flex: 1;
    display: flex;
    overflow: hidden;
}

#graph-panel {
    flex: 1;
    min-width: 0;
}

#cy {
    width: 100%;
    height: 100%;
    background: var(--bg);
}

#context-panel {
    width: 350px;
    background: var(--bg-panel);
    border-left: 1px solid var(--border);
    overflow-y: auto;
    padding: 1rem;
}

#block-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
}

#current-block {
    color: var(--pink);
    font-size: 1.5rem;
    font-family: monospace;
}

.badge {
    background: var(--bg);
    color: var(--accent);
    padding: 0.2rem 0.5rem;
    border-radius: 4px;
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
}

.badge.entry { background: var(--green); color: var(--bg); }
.badge.exit { background: var(--purple); color: var(--bg); }
.badge.branchpoint { background: var(--orange); color: var(--bg); }
.badge.mergepoint { background: var(--accent); color: var(--bg); }
.badge.cleanup { background: #ff5555; color: white; }

#block-summary {
    color: var(--text-dim);
    font-size: 0.9rem;
    margin-bottom: 1rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid var(--border);
}

#context-panel section {
    margin-bottom: 1rem;
}

#context-panel h3 {
    color: var(--text-dim);
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.5rem;
}

#context-panel section.collapsed ul,
#context-panel section.collapsed div:not(h3) {
    display: none;
}

#context-panel section.collapsed h3 {
    cursor: pointer;
}

#stmt-list, #locals-list {
    list-style: none;
    font-family: 'SF Mono', 'Fira Code', monospace;
    font-size: 0.8rem;
}

#stmt-list li, #locals-list li {
    padding: 0.4rem 0;
    border-bottom: 1px solid rgba(255,255,255,0.05);
}

#stmt-list .mir {
    color: var(--green);
}

#stmt-list .annotation {
    color: var(--purple);
    font-size: 0.75rem;
    display: block;
    margin-top: 0.2rem;
}

#term-display {
    font-family: monospace;
    font-size: 0.85rem;
}

#term-display .mir {
    color: var(--pink);
}

#term-display .annotation {
    color: var(--purple);
    font-size: 0.8rem;
    margin-top: 0.3rem;
}

#edge-buttons {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
}

.edge-btn {
    background: var(--bg);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 0.6rem 0.8rem;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    transition: all 0.15s;
}

.edge-btn:hover {
    background: var(--border);
    border-color: var(--accent);
}

.edge-btn .target {
    color: var(--green);
    font-family: monospace;
    font-weight: 600;
}

.edge-btn .label {
    color: var(--orange);
    margin-left: 0.5rem;
    font-size: 0.85rem;
}

.edge-btn .hint {
    display: block;
    color: var(--text-dim);
    font-size: 0.75rem;
    margin-top: 0.2rem;
}

.edge-btn.cleanup {
    border-color: #ff5555;
    border-style: dashed;
}

.edge-btn.cleanup .target {
    color: #ff5555;
}

#alt-paths {
    color: var(--text-dim);
    font-size: 0.85rem;
    padding: 0.5rem;
    background: rgba(255,255,255,0.03);
    border-radius: 4px;
}

#alt-paths:empty {
    display: none;
}

#controls {
    padding: 0.5rem 1.5rem;
    background: var(--bg-panel);
    border-top: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 1rem;
}

#step-counter {
    color: var(--text-dim);
    font-size: 0.85rem;
}

.hint {
    color: var(--text-dim);
    font-size: 0.8rem;
    margin-left: auto;
}
"##;

    /// JavaScript for the standalone explorer (class-based, full-page)
    pub const JS: &str = r##"
class MirExplorer {
    constructor(data) {
        this.data = data;
        this.currentFnIndex = 0;
        this.path = [];
        this.currentBlock = null;

        this.initFunctionSelector();
        this.initGraph();
        this.goToBlock(this.currentFn.entry_block);
        this.initKeyboard();
    }

    get currentFn() {
        return this.data.functions[this.currentFnIndex];
    }

    initFunctionSelector() {
        const selector = document.getElementById('fn-selector');
        if (this.data.functions.length <= 1) return;

        const select = document.createElement('select');
        this.data.functions.forEach((fn, i) => {
            const opt = document.createElement('option');
            opt.value = i;
            opt.textContent = fn.short_name;
            select.appendChild(opt);
        });
        select.onchange = (e) => {
            this.currentFnIndex = parseInt(e.target.value);
            this.reset();
            this.initGraph();
            this.goToBlock(this.currentFn.entry_block);
        };
        selector.appendChild(select);
    }

    initGraph() {
        const elements = this.buildElements();

        if (this.cy) {
            this.cy.destroy();
        }

        this.cy = cytoscape({
            container: document.getElementById('cy'),
            elements: elements,
            style: [
                {
                    selector: 'node',
                    style: {
                        'label': 'data(label)',
                        'text-valign': 'center',
                        'text-halign': 'center',
                        'background-color': '#3a3a5e',
                        'color': '#eee',
                        'font-size': '12px',
                        'font-family': 'monospace',
                        'width': 60,
                        'height': 35,
                        'shape': 'roundrectangle',
                        'border-width': 2,
                        'border-color': '#555'
                    }
                },
                {
                    selector: 'node.entry',
                    style: { 'border-color': '#50fa7b', 'border-width': 3 }
                },
                {
                    selector: 'node.exit',
                    style: { 'border-color': '#bd93f9', 'border-width': 3 }
                },
                {
                    selector: 'node.branchpoint',
                    style: { 'border-color': '#ffb86c', 'border-width': 3 }
                },
                {
                    selector: 'node.visited',
                    style: { 'background-color': '#2a4a6e' }
                },
                {
                    selector: 'node.current',
                    style: {
                        'background-color': '#50fa7b',
                        'color': '#1a1a2e',
                        'border-color': '#50fa7b',
                        'font-weight': 'bold'
                    }
                },
                {
                    selector: 'node.dim',
                    style: { 'opacity': 0.35 }
                },
                {
                    selector: 'edge',
                    style: {
                        'width': 2,
                        'line-color': '#555',
                        'target-arrow-color': '#555',
                        'target-arrow-shape': 'triangle',
                        'curve-style': 'bezier',
                        'label': 'data(label)',
                        'font-size': '10px',
                        'color': '#888',
                        'text-rotation': 'autorotate',
                        'text-margin-y': -10
                    }
                },
                {
                    selector: 'edge.cleanup',
                    style: {
                        'line-style': 'dashed',
                        'line-color': '#ff5555',
                        'target-arrow-color': '#ff5555'
                    }
                },
                {
                    selector: 'edge.taken',
                    style: {
                        'line-color': '#50fa7b',
                        'target-arrow-color': '#50fa7b',
                        'width': 3
                    }
                }
            ],
            layout: {
                name: 'breadthfirst',
                directed: true,
                padding: 50,
                spacingFactor: 1.5
            }
        });

        // Click handlers
        this.cy.on('tap', 'node', (evt) => {
            const id = parseInt(evt.target.id().replace('bb', ''));
            this.goToBlock(id);
        });

        this.cy.on('tap', 'edge', (evt) => {
            const targetId = parseInt(evt.target.target().id().replace('bb', ''));
            this.goToBlock(targetId);
        });
    }

    buildElements() {
        const fn = this.currentFn;
        const nodes = fn.blocks.map(b => ({
            data: { id: `bb${b.id}`, label: `bb${b.id}` },
            classes: b.role
        }));

        const edges = [];
        for (const block of fn.blocks) {
            for (const edge of block.terminator.edges) {
                edges.push({
                    data: {
                        id: `bb${block.id}-bb${edge.target}`,
                        source: `bb${block.id}`,
                        target: `bb${edge.target}`,
                        label: edge.label
                    },
                    classes: edge.kind === 'cleanup' ? 'cleanup' : ''
                });
            }
        }

        return { nodes, edges };
    }

    goToBlock(blockId) {
        const fn = this.currentFn;
        if (blockId < 0 || blockId >= fn.blocks.length) return;

        const block = fn.blocks[blockId];

        // Update path
        if (this.currentBlock !== null && this.currentBlock !== blockId) {
            // Only add to path if not already there (avoid duplicates when going back)
            if (this.path[this.path.length - 1] !== this.currentBlock) {
                this.path.push(this.currentBlock);
            }
        }
        this.currentBlock = blockId;

        // Update graph styling
        this.cy.nodes().removeClass('current visited dim');
        this.cy.edges().removeClass('taken');

        const visitedSet = new Set([...this.path, blockId]);

        // Mark visited
        for (const v of this.path) {
            this.cy.$(`#bb${v}`).addClass('visited');
        }

        // Mark current
        this.cy.$(`#bb${blockId}`).addClass('current');

        // Dim unvisited
        this.cy.nodes().forEach(n => {
            const id = parseInt(n.id().replace('bb', ''));
            if (!visitedSet.has(id)) {
                n.addClass('dim');
            }
        });

        // Mark taken edges
        for (let i = 0; i < this.path.length; i++) {
            const from = this.path[i];
            const to = i + 1 < this.path.length ? this.path[i + 1] : blockId;
            this.cy.$(`#bb${from}-bb${to}`).addClass('taken');
        }

        // Center on current node
        this.cy.animate({
            center: { eles: this.cy.$(`#bb${blockId}`) },
            duration: 200
        });

        // Update UI
        this.updateContextPanel(block);
        this.updatePathBreadcrumb();
        this.updateStepCounter();
    }

    goBack() {
        if (this.path.length > 0) {
            const prev = this.path.pop();
            this.currentBlock = null; // Reset to avoid double-push
            this.goToBlock(prev);
        }
    }

    reset() {
        this.path = [];
        this.currentBlock = null;
        if (this.cy) {
            this.cy.nodes().removeClass('current visited dim');
            this.cy.edges().removeClass('taken');
        }
    }

    updateContextPanel(block) {
        // Header
        document.getElementById('current-block').textContent = `bb${block.id}`;
        const badge = document.getElementById('block-role');
        badge.textContent = block.role.toUpperCase();
        badge.className = 'badge ' + block.role;

        // Summary
        document.getElementById('block-summary').textContent = block.summary;

        // Locals (collapsed by default)
        const localsList = document.getElementById('locals-list');
        localsList.innerHTML = this.currentFn.locals.map(l => renderLocalHtml(l, escapeHtml)).join('');

        // Statements
        const stmtList = document.getElementById('stmt-list');
        stmtList.innerHTML = '';
        if (block.statements.length === 0) {
            const li = document.createElement('li');
            li.innerHTML = '<span class="mir" style="color: var(--text-dim);">(no statements)</span>';
            stmtList.appendChild(li);
        } else {
            for (const stmt of block.statements) {
                const li = document.createElement('li');
                li.innerHTML = `
                    <span class="mir">${escapeHtml(stmt.mir)}</span>
                    <span class="annotation">${escapeHtml(stmt.annotation)}</span>
                `;
                stmtList.appendChild(li);
            }
        }

        // Terminator
        const termDisplay = document.getElementById('term-display');
        termDisplay.innerHTML = `
            <div class="mir">${escapeHtml(block.terminator.mir)}</div>
            <div class="annotation">${escapeHtml(block.terminator.annotation)}</div>
        `;

        // Edge buttons
        const edgeContainer = document.getElementById('edge-buttons');
        edgeContainer.innerHTML = '';
        for (const edge of block.terminator.edges) {
            const btn = document.createElement('button');
            btn.className = 'edge-btn' + (edge.kind === 'cleanup' ? ' cleanup' : '');
            btn.innerHTML = `
                <span class="target">→ bb${edge.target}</span>
                <span class="label">${escapeHtml(edge.label)}</span>
                <span class="hint">${escapeHtml(edge.annotation)}</span>
            `;
            btn.onclick = () => this.goToBlock(edge.target);
            edgeContainer.appendChild(btn);
        }

        // Alternative paths
        const altPaths = document.getElementById('alt-paths');
        const otherPreds = block.predecessors.filter(p =>
            !this.path.includes(p) && p !== this.path[this.path.length - 1]
        );
        if (otherPreds.length > 0 && this.path.length > 0) {
            altPaths.innerHTML = `
                <strong>Also reachable from:</strong>
                ${otherPreds.map(p => `bb${p}`).join(', ')}
            `;
        } else {
            altPaths.innerHTML = '';
        }
    }

    updatePathBreadcrumb() {
        const crumb = document.getElementById('path-breadcrumb');
        const fullPath = [...this.path, this.currentBlock];
        crumb.innerHTML = fullPath.map((b, i) => {
            const isLast = i === fullPath.length - 1;
            return `<span class="${isLast ? 'current' : 'visited'}">bb${b}</span>`;
        }).join(' → ');
    }

    updateStepCounter() {
        document.getElementById('step-counter').textContent =
            `Step ${this.path.length + 1}`;
    }

    initKeyboard() {
        document.addEventListener('keydown', (e) => {
            if (e.key === 'ArrowLeft' || e.key === 'Backspace') {
                e.preventDefault();
                this.goBack();
            }
            // Number keys 1-9 for quick edge selection
            if (e.key >= '1' && e.key <= '9') {
                const block = this.currentFn.blocks[this.currentBlock];
                const idx = parseInt(e.key) - 1;
                if (block.terminator.edges[idx]) {
                    this.goToBlock(block.terminator.edges[idx].target);
                }
            }
        });
    }
}

function escapeHtml(s) {
    if (!s) return '';
    return s.replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;');
}

function toggleSection(sectionId) {
    const section = document.getElementById(sectionId);
    section.classList.toggle('collapsed');
    const h3 = section.querySelector('h3');
    if (section.classList.contains('collapsed')) {
        h3.textContent = h3.textContent.replace('▾', '▸');
    } else {
        h3.textContent = h3.textContent.replace('▸', '▾');
    }
}

// Initialize
const explorer = new MirExplorer(EXPLORER_DATA);
"##;
}

// =============================================================================
// Embedded Explorer Assets (--html flag)
// =============================================================================

/// Assets for the explorer panels embedded in the annotated HTML output
pub mod embedded {
    /// JavaScript for embedded explorer panels (function-based, multiple instances)
    pub const EXPLORER_JS: &str = r##"
function initExplorer(id, data) {
    const cy = cytoscape({
        container: document.getElementById(`cy-${id}`),
        elements: buildElements(data),
        style: [
            { selector: 'node', style: {
                'label': 'data(label)',
                'text-valign': 'center',
                'text-halign': 'center',
                'background-color': '#3a3a5e',
                'color': '#eee',
                'font-size': '11px',
                'font-family': 'monospace',
                'width': 50,
                'height': 30,
                'shape': 'roundrectangle',
                'border-width': 2,
                'border-color': '#555'
            }},
            { selector: 'node.entry', style: { 'border-color': '#50fa7b', 'border-width': 3 }},
            { selector: 'node.exit', style: { 'border-color': '#bd93f9', 'border-width': 3 }},
            { selector: 'node.branchpoint', style: { 'border-color': '#ffb86c', 'border-width': 3 }},
            { selector: 'node.visited', style: { 'background-color': '#2a4a6e' }},
            { selector: 'node.current', style: {
                'background-color': '#50fa7b',
                'color': '#1a1a2e',
                'border-color': '#50fa7b',
                'font-weight': 'bold'
            }},
            { selector: 'node.dim', style: { 'opacity': 0.35 }},
            { selector: 'edge', style: {
                'width': 2,
                'line-color': '#555',
                'target-arrow-color': '#555',
                'target-arrow-shape': 'triangle',
                'curve-style': 'bezier',
                'label': 'data(label)',
                'font-size': '9px',
                'color': '#888'
            }},
            { selector: 'edge.cleanup', style: {
                'line-style': 'dashed',
                'line-color': '#ff5555',
                'target-arrow-color': '#ff5555'
            }},
            { selector: 'edge.taken', style: {
                'line-color': '#50fa7b',
                'target-arrow-color': '#50fa7b',
                'width': 3
            }}
        ],
        layout: { name: 'breadthfirst', directed: true, padding: 30, spacingFactor: 1.2 }
    });

    const state = { path: [], current: null, data: data, cy: cy, id: id };
    explorers[id] = {
        goTo: (blockId) => goToBlock(state, blockId),
        goBack: () => goBack(state),
        reset: () => resetExplorer(state)
    };

    cy.on('tap', 'node', (e) => goToBlock(state, parseInt(e.target.id().replace('bb', ''))));
    cy.on('tap', 'edge', (e) => goToBlock(state, parseInt(e.target.target().id().replace('bb', ''))));

    goToBlock(state, data.entry_block);
}

function buildElements(data) {
    const nodes = data.blocks.map(b => ({
        data: { id: `bb${b.id}`, label: `bb${b.id}` },
        classes: b.role
    }));
    const edges = [];
    for (const block of data.blocks) {
        for (const edge of block.terminator.edges) {
            edges.push({
                data: {
                    id: `bb${block.id}-bb${edge.target}`,
                    source: `bb${block.id}`,
                    target: `bb${edge.target}`,
                    label: edge.label
                },
                classes: edge.kind === 'cleanup' ? 'cleanup' : ''
            });
        }
    }
    return { nodes, edges };
}

function goToBlock(state, blockId) {
    const { data, cy, id } = state;
    if (blockId < 0 || blockId >= data.blocks.length) return;
    const block = data.blocks[blockId];

    if (state.current !== null && state.current !== blockId) {
        if (state.path[state.path.length - 1] !== state.current) {
            state.path.push(state.current);
        }
    }
    state.current = blockId;

    cy.nodes().removeClass('current visited dim');
    cy.edges().removeClass('taken');
    const visited = new Set([...state.path, blockId]);
    state.path.forEach(v => cy.$(`#bb${v}`).addClass('visited'));
    cy.$(`#bb${blockId}`).addClass('current');
    cy.nodes().forEach(n => {
        if (!visited.has(parseInt(n.id().replace('bb', '')))) n.addClass('dim');
    });
    for (let i = 0; i < state.path.length; i++) {
        const from = state.path[i], to = i + 1 < state.path.length ? state.path[i + 1] : blockId;
        cy.$(`#bb${from}-bb${to}`).addClass('taken');
    }
    cy.animate({ center: { eles: cy.$(`#bb${blockId}`) }, duration: 150 });

    updateContext(state, block);
    updatePath(state);
}

function goBack(state) {
    if (state.path.length > 0) {
        const prev = state.path.pop();
        state.current = null;
        goToBlock(state, prev);
    }
}

function resetExplorer(state) {
    state.path = [];
    state.current = null;
    state.cy.nodes().removeClass('current visited dim');
    state.cy.edges().removeClass('taken');
    goToBlock(state, state.data.entry_block);
}

function updateContext(state, block) {
    const { id, data } = state;
    document.getElementById(`block-${id}`).textContent = `bb${block.id}`;
    const badge = document.getElementById(`role-${id}`);
    badge.textContent = block.role;
    badge.className = `badge ${block.role}`;
    document.getElementById(`summary-${id}`).textContent = block.summary;

    // Populate locals
    const localsList = document.getElementById(`locals-${id}`);
    localsList.innerHTML = data.locals.map(l => renderLocalHtml(l, escapeHtml)).join('');

    const stmts = document.getElementById(`stmts-${id}`);
    stmts.innerHTML = block.statements.length === 0
        ? '<li style="color:#888">(none)</li>'
        : block.statements.map(s =>
            `<li><span class="mir">${escapeHtml(s.mir)}</span><span class="annotation">${escapeHtml(s.annotation)}</span></li>`
          ).join('');

    document.getElementById(`term-${id}`).innerHTML =
        `<span class="term">${escapeHtml(block.terminator.mir)}</span><span class="annotation">${escapeHtml(block.terminator.annotation)}</span>`;

    const edges = document.getElementById(`edges-${id}`);
    edges.innerHTML = block.terminator.edges.map(e => {
        const cleanupClass = e.kind === 'cleanup' ? ' cleanup' : '';
        const labelSpan = e.label ? `<span class="label">${escapeHtml(e.label)}</span>` : '';
        return `<button class="edge-btn${cleanupClass}" onclick="explorers['${id}'].goTo(${e.target})">
            <span class="target">→ bb${e.target}</span> ${labelSpan}
            <span class="hint">${escapeHtml(e.annotation)}</span></button>`;
    }).join('');
}

function updatePath(state) {
    const crumb = document.querySelector(`#path-${state.id} .breadcrumb`);
    const full = [...state.path, state.current];
    crumb.innerHTML = full.map((b, i) => {
        const currentClass = i === full.length - 1 ? ' current' : '';
        return `<span class="crumb${currentClass}">bb${b}</span>`;
    }).join(' → ');
}

function escapeHtml(s) {
    if (!s) return '';
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
"##;
}
