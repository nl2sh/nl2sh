#!/usr/bin/env python3
"""Render or check code exports inside bilingual Markdown reference sections."""
import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
START, END = '<!-- generated:start -->', '<!-- generated:end -->'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('export', type=Path, help='JSON from cargo run --example docs_export')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    data = json.loads(args.export.read_text())
    translations = json.loads((ROOT / 'docs/_data/reference-translations.json').read_text())
    source = (ROOT / 'src/config/model.rs').read_text().split('pub struct Config {', 1)[1].split('\n}', 1)[0]
    fields = re.findall(r'/// ([^\n]+)\n\s*pub (\w+):', source)
    stale = []
    def output(path, content):
        content = content.rstrip() + '\n'
        if args.check:
            if not path.exists() or path.read_text() != content:
                stale.append(str(path.relative_to(ROOT)))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
    for lang in ('zh', 'en'):
        config = '| 字段 | 默认值 | 说明 |\n| --- | --- | --- |\n' if lang == 'zh' else '| Field | Default | Description |\n| --- | --- | --- |\n'
        for desc, name in fields:
            if name == 'source':
                continue
            value = data['defaults'].get(name, 'auto' if name == 'api_type' else None)
            desc = translations['config'][name] if lang == 'zh' else desc
            config += f'| `{name}` | `{json.dumps(value, ensure_ascii=False)}` | {desc} |\n'
        cli = '\n\n'.join('```text\n' + item.rstrip() + '\n```' for item in data['cli'])
        tools = []
        for tool in sorted(data['tools'], key=lambda t: t['name']):
            desc = translations['tools'][tool['name']] if lang == 'zh' else tool['description']
            schema = json.dumps(tool['parameters'], ensure_ascii=False, indent=2)
            tools.append(f"## `{tool['name']}`\n\n{desc}\n\n```json\n{schema}\n```")
        for name, body in [('configuration', config), ('cli', cli), ('tool-catalog', '\n\n'.join(tools))]:
            path = ROOT / f'docs/{lang}/reference/{name}.md'
            text = path.read_text()
            before, rest = text.split(START, 1)
            _, after = rest.split(END, 1)
            output(path, before + START + '\n\n' + body.rstrip() + '\n\n' + END + after)
    output(ROOT / 'docs/assets/tool-schemas.json', json.dumps(data['tools'], ensure_ascii=False, indent=2))
    if stale:
        print('Stale generated reference: ' + ', '.join(stale), file=sys.stderr)
        return 1
    print('Code-derived bilingual reference is current.' if args.check else 'Updated bilingual reference.')
    return 0

if __name__ == '__main__':
    sys.exit(main())
