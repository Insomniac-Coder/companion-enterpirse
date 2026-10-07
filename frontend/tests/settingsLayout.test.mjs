import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
import ts from 'typescript';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const sourcePath = fileURLToPath(new URL('../src/components/SettingsPanel.tsx', import.meta.url));
const result = await build({ entryPoints: [sourcePath], bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', external: ['react', 'react/*'], loader: { '.css': 'empty' } });
const loaded = { exports: {} };
new Function('require', 'module', 'exports', result.outputFiles[0].text)(createRequire(import.meta.url), loaded, loaded.exports);
const { SettingField, HardwareOverrides, RuntimeSummary, shownNumber } = loaded.exports;
const personalPath = fileURLToPath(new URL('../src/components/PersonalSettings.tsx', import.meta.url));
const personalBuilt = await build({ entryPoints: [personalPath], bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', external: ['react', 'react/*'], loader: { '.css': 'empty' } });
const personal = { exports: {} };
new Function('require', 'module', 'exports', personalBuilt.outputFiles[0].text)(createRequire(import.meta.url), personal, personal.exports);

test('loaded runtime exposes CPU fallback explanation rather than only next-load settings', () => {
  const cpu = { architecture: 'test', weights_quantization: 'Q4', effective_context: 8192, cache_type_k: 'f16', cache_type_v: 'f16', threads: 0, gpu_layers: 0, batch_size: 128, flash_attention: 'off', kv_offload: 'off', notes: ['CPU mode selected automatically: no usable GPU.'] };
  const html = renderToStaticMarkup(createElement(RuntimeSummary, { policy: { active: cpu, next: { ...cpu, notes: [] } }, dirty: false }));
  assert.match(html, /Loaded session[\s\S]*CPU mode selected automatically: no usable GPU/);
  assert.match(html, /GPU layers<\/dt><dd>0<\/dd>/);
});

test('optional Auto badge stays inside the same control row and the label remains linked', () => {
  const html = renderToStaticMarkup(createElement(SettingField, { label: 'GPU layers (-1 auto)' }, createElement('input', { type: 'number', defaultValue: -1 }), createElement('span', null, 'Auto')));
  const labelId = /<label[^>]*for="([^"]+)"/.exec(html)?.[1];
  assert.ok(labelId);
  assert.ok(html.includes(`id="${labelId}"`));
  assert.match(html, /class="settings-field-control"><input[^>]+><span>Auto<\/span><\/div>/);
});

test('checkbox settings have their own row and clickable native label', () => {
  const html = renderToStaticMarkup(createElement(SettingField, { label: 'Flash attention' }, createElement('input', { type: 'checkbox', defaultChecked: true })));
  assert.match(html, /settings-field--toggle/);
  const id = /for="([^"]+)"/.exec(html)?.[1];
  assert.ok(id && html.includes(`id="${id}"`));
  assert.match(html, /checked=""/);
});

test('default-model select, refresh action and help stay in one accessible field', () => {
  const html = renderToStaticMarkup(createElement(SettingField, { label: 'Default model', description: 'Changes apply after Save.' },
    createElement('select', { defaultValue: 'missing-id' }, createElement('option', { value: '' }, 'Automatic selection'), createElement('option', { value: 'missing-id' }, 'Previously selected model unavailable')),
    createElement('button', { type: 'button' }, 'Refresh')));
  const id = /<label[^>]*for="([^"]+)"/.exec(html)?.[1];
  assert.ok(id && html.includes(`id="${id}"`));
  assert.ok(html.includes(`aria-describedby="${id}-description"`));
  assert.ok(html.includes(`id="${id}-description"`));
  assert.match(html, /<option value="missing-id" selected="">Previously selected model unavailable/);
  assert.match(html, /<\/select><button type="button">Refresh<\/button><\/div>/);
});

function fieldSections(path) {
  const source = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  let sectionCount = 0;
  function visit(node) {
    if (ts.isJsxElement(node) && node.openingElement.attributes.properties.some((attribute) => ts.isJsxAttribute(attribute) && attribute.name.getText(source) === 'className' && attribute.initializer?.text === 'settings-fields')) {
      sectionCount++;
      for (const child of node.children) {
        if (ts.isJsxText(child) && !child.text.trim()) continue;
        const field = ts.isJsxExpression(child) && child.expression && ts.isBinaryExpression(child.expression) ? child.expression.right : child;
        assert.ok(ts.isJsxElement(field) && field.openingElement.tagName.getText(source) === 'SettingField', 'all fields must be contained in one row');
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return sectionCount;
}

test('every settings section contains explicit field rows, including conditional fields', () => {
  // The dashboard's startup model, a model's settings, web search, rules and privacy, plus the manual
  // hardware overrides.
  assert.equal(fieldSections(sourcePath), 6);
  // Companion's own: general, chats, memory and context, code and agents, keyboard.
  assert.equal(fieldSections(personalPath), 5);
});

test('a person never chooses a permission mode above what the company allows', () => {
  const html = renderToStaticMarkup(createElement(personal.exports.PersonalSection, { id: 'agents', s: { agent: { permission_mode: 'ask', max_permission_mode: 'accept_edits' } }, set() {} }));
  assert.match(html, /<option value="accept_edits">/);
  assert.match(html, /<option value="auto" disabled="">/);
});

test('hardware overrides are hidden by default and return with preserved values only in manual mode', () => {
  const settings = { hardware: { cpu_threads: 7, gpu_layers: -1, flash_attention: true, kv_cache_gpu: true }, inference: { batch_size: 256 } };
  assert.equal(renderToStaticMarkup(createElement(HardwareOverrides, { settings, set() {} })), '');
  assert.equal(renderToStaticMarkup(createElement(HardwareOverrides, { settings: { ...settings, runtime_auto: true }, set() {} })), '');
  const html = renderToStaticMarkup(createElement(HardwareOverrides, { settings: { ...settings, runtime_auto: false }, set() {} }));
  assert.match(html, /CPU threads/);
  assert.match(html, /value="7"/);
  assert.match(html, /Prompt batch size/);
  assert.match(html, /value="256"/);
});

test('runtime details separate requested configuration from measurements and unsaved changes', () => {
  const resolved = { mode: 'automatic', architecture: 'test', weights_quantization: 'Q4_K_M', effective_context: 32768, cache_type_k: 'f16', cache_type_v: 'f16', threads: 0, gpu_layers: -1, batch_size: 512, flash_attention: 'auto', kv_offload: 'auto', notes: [] };
  const policy = { running: true, model_name: 'Loaded model', active: resolved, next: { ...resolved, effective_context: 8192 } };
  const html = renderToStaticMarkup(createElement(RuntimeSummary, { policy, dirty: true }));
  assert.match(html, /not measurements of memory use/);
  assert.match(html, /Runtime managed/);
  assert.match(html, /Save your changes/);
  assert.doesNotMatch(html, /8,192/);
  const saved = renderToStaticMarkup(createElement(RuntimeSummary, { policy, dirty: false }));
  assert.match(saved, /8,192/);
});

test('inactive preferences are not exposed as editable settings', () => {
  const source = readFileSync(sourcePath, 'utf8') + readFileSync(personalPath, 'utf8');
  // Saved but never applied: showing them would promise something the app does not do.
  for (const key of ['output_dir', 'telemetry', 'server_port', 'kv_cache_type', 'log_level', 'confirm_outside_copy', 'default_dir', 'share_across_modes', 'ocr_enabled']) {
    assert.equal(source.includes(`'${key}'`), false, `${key} must not be editable`);
  }
  // Applied, and so real settings: compaction (chat before a reply, the agent between steps), and
  // the rules the dashboard sets (folders, attachments, the command limit, log masking).
  for (const key of ['auto_compact', 'compact_at_pct', 'allowed_dirs', 'blocked_dirs', 'max_attach_mb', 'command_timeout_secs', 'log_redaction']) {
    assert.ok(source.includes(`'${key}'`), `${key} has a control`);
  }
  assert.match(source, /saved custom provider is not implemented/);
});

test('numbers read as people wrote them, not as 32-bit float noise', () => {
  assert.equal(shownNumber(0.30000001192092896), 0.3);
  assert.equal(shownNumber(1.100000023841858), 1.1);
  assert.equal(shownNumber(32768), 32768);
  assert.equal(shownNumber(undefined), '');
});
