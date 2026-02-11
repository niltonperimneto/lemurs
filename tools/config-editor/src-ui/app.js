const { invoke } = window.__TAURI__.tauri;

const state = {
    config: null,
    currentCat: 'general'
};

const dom = {
    navItems: document.querySelectorAll('.nav-item'),
    formContainer: document.getElementById('form-container'),
    sectionTitle: document.getElementById('section-title'),
    btnSave: document.getElementById('btn-save'),
    btnReload: document.getElementById('btn-reload'),
    preview: document.getElementById('tui-preview')
};

// Initial Load
document.addEventListener('DOMContentLoaded', async () => {
    try {
        await loadConfig();
    } catch (e) {
        console.error(e);
        alert('Failed to load config: ' + e);
        // Fallback for UI testing if backend not ready
        if (!state.config) {
            state.config = mockConfig();
            renderForm();
            updatePreview();
        }
    }
});

dom.navItems.forEach(el => {
    el.addEventListener('click', () => {
        dom.navItems.forEach(n => n.classList.remove('active'));
        el.classList.add('active');
        state.currentCat = el.dataset.cat;
        renderForm();
    });
});

dom.btnSave.addEventListener('click', async () => {
    try {
        // Collect data from form?
        // Actually, we bind inputs to state.config directly
        await invoke('save_config', { config: state.config });
        alert('Saved successfully!');
    } catch (e) {
        alert('Save failed: ' + e);
    }
});

dom.btnReload.addEventListener('click', loadConfig);

async function loadConfig() {
    state.config = await invoke('get_config');
    renderForm();
    updatePreview();
}

function renderForm() {
    const cat = state.currentCat;
    dom.sectionTitle.textContent = cat.charAt(0).toUpperCase() + cat.slice(1);
    dom.formContainer.innerHTML = '';

    const schema = getSchema(cat);

    schema.forEach(field => {
        const group = document.createElement('div');
        group.className = 'form-group';

        const label = document.createElement('label');
        label.textContent = field.label;

        const input = document.createElement('input');

        // Bind value
        const val = getNested(state.config, field.key);

        if (field.type === 'boolean') {
            input.type = 'checkbox';
            input.checked = val;
            input.addEventListener('change', (e) => {
                setNested(state.config, field.key, e.target.checked);
                updatePreview();
            });
        } else {
            input.type = 'text';
            input.value = val;
            input.addEventListener('input', (e) => {
                setNested(state.config, field.key, e.target.value);
                updatePreview();
            });
        }

        group.appendChild(label);
        group.appendChild(input);
        dom.formContainer.appendChild(group);
    });
}

function updatePreview() {
    const p = dom.preview;
    p.innerHTML = '';

    const layout = state.config?.design?.layout || 'boxed';
    const showClock = state.config?.design?.show_clock;

    // Background
    if (state.config?.background?.image) {
        // p.style.backgroundImage = `url(${state.config.background.image})`; 
        // Cannot easy access file system images without protocol
    }
    p.style.backgroundColor = state.config?.background?.style?.color || '#000';

    const clockHtml = showClock ? `<div class="tui-clock">12:34</div>` : '';
    const inputsHtml = `
        <div class="tui-input">[ Username ]</div>
        <div class="tui-input">[ Password ]</div>
    `;

    if (layout === 'minimal') {
        p.innerHTML = `
            ${clockHtml}
            <div style="margin-top:auto; display:flex; flex-direction:column; align-items:center; padding-bottom: 50px;">
                ${inputsHtml}
            </div>
        `;
    } else {
        // Boxed
        p.innerHTML = `
            <div class="tui-box" style="border: 2px solid #fff;">
                ${clockHtml}
                ${inputsHtml}
            </div>
        `;
    }
}

// ---- Helpers ----

function getNested(obj, path) {
    if (!obj) return '';
    return path.split('.').reduce((o, i) => o ? o[i] : '', obj);
}

function setNested(obj, path, val) {
    if (!obj) return;
    const keys = path.split('.');
    const last = keys.pop();
    const target = keys.reduce((o, i) => o[i], obj);
    if (target) target[last] = val;
}

function mockConfig() {
    return {
        tty: 2,
        design: { layout: 'minimal', show_clock: true },
        background: { style: { color: '#1e1e2e' } },
        panel: { show_panel: true }
    };
}

function getSchema(cat) {
    // Simple mapping for demo
    const map = {
        general: [
            { key: 'tty', label: 'Monitor TTY', type: 'number' },
            { key: 'pam_service', label: 'PAM Service', type: 'text' }
        ],
        design: [
            { key: 'design.layout', label: 'Layout (boxed/minimal)', type: 'text' },
            { key: 'design.show_clock', label: 'Show Clock', type: 'boolean' }
        ],
        background: [
            { key: 'background.show_background', label: 'Enable Background', type: 'boolean' },
            { key: 'background.style.color', label: 'Background Color', type: 'text' },
            { key: 'background.image', label: 'Image Path', type: 'text' }
        ],
        panel: [
            { key: 'panel.show_panel', label: 'Show Panel', type: 'boolean' },
            { key: 'panel.position', label: 'Position', type: 'text' }
        ],
        power: [
            // Complex logic skipped for now
        ]
    };
    return map[cat] || [];
}
